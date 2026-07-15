const el = (id) => document.getElementById(id);
const statusEl = el("status");
const idEl = el("id");
const accelEl = el("accel");
const accelCountEl = el("accel-count");
const gpsEl = el("gps");
const gpsCountEl = el("gps-count");
const startBtn = el("start");

const DEVICE_ID_COOKIE = "lookout_device_id";

// Raw sensor events fire far faster than we want to record; we keep only the latest
// reading from each source and emit one sample per source on this fixed interval.
const SAMPLE_INTERVAL_MS = 10000;

let accelCount = 0;
let gpsCount = 0;

// Latest unread reading from each source, consumed (and cleared) on each sample tick.
let pendingAccel = null;
let pendingGps = null;

// Fire each source's first sample as soon as it produces a reading, rather than
// waiting a full SAMPLE_INTERVAL_MS for the interval's first tick. Tracked per source
// so a first reading from one doesn't suppress the other's leading sample.
let firstAccelSampled = false;
function takeFirstAccelSample() {
  if (firstAccelSampled) return;
  firstAccelSampled = true;
  emitAccelSample();
}

let firstGpsSampled = false;
function takeFirstGpsSample() {
  if (firstGpsSampled) return;
  firstGpsSampled = true;
  emitGpsSample();
}

function setStatus(text) {
  statusEl.textContent = text;
}

function getCookie(name) {
  return document.cookie
    .split("; ")
    .find((row) => row.startsWith(`${name}=`))
    ?.split("=")[1];
}

function setCookie(name, value) {
  const oneYear = 60 * 60 * 24 * 365;
  document.cookie = `${name}=${value}; max-age=${oneYear}; path=/; SameSite=Strict`;
}

// Stable per-device identity, generated once and persisted in a cookie.
function deviceId() {
  let id = getCookie(DEVICE_ID_COOKIE);
  if (!id) {
    id = crypto.randomUUID();
    setCookie(DEVICE_ID_COOKIE, id);
  }
  return id;
}

const id = deviceId();
idEl.textContent = id;

// Sensor events only stash their latest reading; sampleTick turns them into samples.
function onMotion(event) {
  const a = event.accelerationIncludingGravity || event.acceleration || {};
  pendingAccel = { x: a.x ?? null, y: a.y ?? null, z: a.z ?? null };
  takeFirstAccelSample();
}

function onPosition(position) {
  const c = position.coords;
  pendingGps = { lat: c.latitude, lon: c.longitude, alt: c.altitude, acc: c.accuracy };
  takeFirstGpsSample();
}

function onPositionError(err) {
  gpsEl.textContent = `error: ${err.message}`;
}

// A sample carries the device id, a timestamp, and either an accel or gps reading.
function emitAccelSample() {
  if (!pendingAccel) return;
  const sample = { id, t: Date.now(), accel: pendingAccel };
  pendingAccel = null;
  accelCount += 1;
  accelCountEl.textContent = String(accelCount);
  accelEl.textContent = JSON.stringify(sample.accel, null, 2);
}

function emitGpsSample() {
  if (!pendingGps) return;
  const sample = { id, t: Date.now(), gps: pendingGps };
  pendingGps = null;
  gpsCount += 1;
  gpsCountEl.textContent = String(gpsCount);
  gpsEl.textContent = JSON.stringify(sample.gps, null, 2);
}

function sampleTick() {
  emitAccelSample();
  emitGpsSample();
}

async function start() {
  startBtn.disabled = true;

  // Safari on iOS requires an explicit, user-gesture-triggered permission grant.
  if (typeof DeviceMotionEvent?.requestPermission === "function") {
    try {
      const result = await DeviceMotionEvent.requestPermission();
      if (result !== "granted") {
        setStatus("motion permission denied");
        startBtn.disabled = false;
        return;
      }
    } catch (err) {
      setStatus(`motion permission error: ${err}`);
      startBtn.disabled = false;
      return;
    }
  }

  window.addEventListener("devicemotion", onMotion);

  if (navigator.geolocation) {
    navigator.geolocation.watchPosition(onPosition, onPositionError, {
      enableHighAccuracy: true,
      maximumAge: 0,
    });
  } else {
    gpsEl.textContent = "geolocation unavailable";
  }

  setInterval(sampleTick, SAMPLE_INTERVAL_MS);
  setStatus("gathering");
}

startBtn.addEventListener("click", start);
