const el = (id) => document.getElementById(id);
const statusEl = el("status");
const idEl = el("id");
const accelEl = el("accel");
const accelCountEl = el("accel-count");
const gpsEl = el("gps");
const gpsCountEl = el("gps-count");
const startBtn = el("start");

const DEVICE_ID_COOKIE = "lookout_device_id";

let accelCount = 0;
let gpsCount = 0;

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

// A sample carries the device id, a timestamp, and either an accel or gps reading.
// Phase 1 only gathers and displays these — it does not send them anywhere yet.
function onMotion(event) {
  const a = event.accelerationIncludingGravity || event.acceleration || {};
  const sample = {
    id,
    t: Date.now(),
    accel: { x: a.x ?? null, y: a.y ?? null, z: a.z ?? null },
  };
  accelCount += 1;
  accelCountEl.textContent = String(accelCount);
  accelEl.textContent = JSON.stringify(sample.accel, null, 2);
}

function onPosition(position) {
  const c = position.coords;
  const sample = {
    id,
    t: Date.now(),
    gps: { lat: c.latitude, lon: c.longitude, alt: c.altitude, acc: c.accuracy },
  };
  gpsCount += 1;
  gpsCountEl.textContent = String(gpsCount);
  gpsEl.textContent = JSON.stringify(sample.gps, null, 2);
}

function onPositionError(err) {
  gpsEl.textContent = `error: ${err.message}`;
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

  setStatus("gathering");
}

startBtn.addEventListener("click", start);
