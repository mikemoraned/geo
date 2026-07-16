const el = (id) => document.getElementById(id);
const statusEl = el("status");
const wakeLockEl = el("wakelock");
const idEl = el("id");
const accelEl = el("accel");
const accelCountEl = el("accel-count");
const gpsEl = el("gps");
const gpsCountEl = el("gps-count");
const startBtn = el("start");

// True once the user has started a session; gates wake-lock re-acquisition.
let recording = false;

const DEVICE_ID_COOKIE = "lookout_device_id";

// Raw sensor events fire far faster than we want to record; we keep only the latest
// reading from each source and emit one sample per source on this fixed interval.
const SAMPLE_INTERVAL_MS = 10000;

let accelCount = 0;
let gpsCount = 0;

// Accelerometer readings arrive at ~60 Hz. Rather than keep only the latest (at 0.1 Hz
// an instantaneous sample just measures gravity), aggregate the gravity-removed
// magnitude across the window and emit { rms, peak, n } on the tick. A single raw
// instantaneous reading is kept for a tilt view. Accumulators reset each emit.
let accelSumSq = 0;
let accelPeak = 0;
let accelN = 0;
let lastAccel = null;

// Latest unread gps fix, consumed (and cleared) on each sample tick, plus the fix's
// own timestamp (0–10 s old — used as the sample `t` instead of Date.now()).
let pendingGps = null;
let pendingGpsT = null;

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

// Wire protocol version. v1 is self-describing: every message carries `v` and a
// `type` tag; v0 (untagged, inferred from the sensor key) is read-only history.
const WIRE_VERSION = 1;

// Classify the device from what Safari exposes. Safari has no UA client hints
// (`navigator.userAgentData` is Chromium-only), so `platform` + `userAgent` are all
// we get. Modern iPadOS reports "MacIntel" with a touch screen, which is how it's
// told apart from a laptop.
function deviceInfo() {
  const platform = navigator.platform || "";
  const userAgent = navigator.userAgent || "";
  const touch = navigator.maxTouchPoints || 0;

  const isIphone = /iPhone/.test(platform) || /iPhone/.test(userAgent);
  const isIpad =
    /iPad/.test(platform) ||
    /iPad/.test(userAgent) ||
    (platform === "MacIntel" && touch > 1);

  let deviceType = "unknown";
  let os = null;
  if (isIphone) {
    deviceType = "iphone";
    os = "iOS";
  } else if (isIpad) {
    deviceType = "ipad";
    os = "iOS";
  } else if (/Mac/.test(platform)) {
    deviceType = "laptop";
    os = "macOS";
  }

  // iOS reports "OS 18_5", macOS "Mac OS X 10_15_7"; normalise underscores to dots.
  const version = userAgent.match(/OS (\d+[_.]\d+(?:[_.]\d+)?)/);
  const osVersion = version ? version[1].replace(/_/g, ".") : null;

  return {
    device_type: deviceType,
    platform,
    user_agent: userAgent,
    os,
    os_version: osVersion,
  };
}

// Accelerometer events accumulate into the window; sampleTick emits the aggregate.
// iOS-only, so `event.acceleration` (gravity-removed) is always present — no
// accelerationIncludingGravity fallback. Its magnitude is orientation-invariant, so
// device placement doesn't matter.
function onMotion(event) {
  const a = event.acceleration || {};
  const x = a.x ?? null;
  const y = a.y ?? null;
  const z = a.z ?? null;
  lastAccel = { x, y, z };
  const mag = Math.hypot(x ?? 0, y ?? 0, z ?? 0);
  accelSumSq += mag * mag;
  accelPeak = Math.max(accelPeak, mag);
  accelN += 1;
  takeFirstAccelSample();
}

function onPosition(position) {
  const c = position.coords;
  // speed (Doppler, m/s) and heading (course, degrees) are nullable; keep the nulls.
  pendingGps = {
    lat: c.latitude,
    lon: c.longitude,
    alt: c.altitude,
    acc: c.accuracy,
    speed: c.speed,
    heading: c.heading,
  };
  pendingGpsT = position.timestamp;
  takeFirstGpsSample();
}

// watchPosition keeps trying after an error, so POSITION_UNAVAILABLE (Core Location's
// kCLErrorLocationUnknown) is usually a transient "no fix yet" — don't treat it as fatal
// or let it clobber a fix we already have.
function onPositionError(err) {
  if (err.code === err.PERMISSION_DENIED) {
    gpsEl.textContent = "permission denied";
  } else if (gpsCount === 0) {
    gpsEl.textContent = "waiting for a gps fix…";
  }
}

// A sample is a v1 message: the wire version, a type tag, the device id, a
// timestamp, and either an accel or gps reading.
function emitAccelSample() {
  if (accelN === 0) return;
  const accel = {
    rms: Math.sqrt(accelSumSq / accelN),
    peak: accelPeak,
    n: accelN,
    x: lastAccel?.x ?? null,
    y: lastAccel?.y ?? null,
    z: lastAccel?.z ?? null,
  };
  accelSumSq = 0;
  accelPeak = 0;
  accelN = 0;
  const sample = { v: WIRE_VERSION, type: "acceleration", id, t: Date.now(), accel };
  accelCount += 1;
  accelCountEl.textContent = String(accelCount);
  accelEl.textContent = JSON.stringify(sample.accel, null, 2);
  sendSample(sample);
}

function emitGpsSample() {
  if (!pendingGps) return;
  // Stamp the fix's own time, not Date.now(): a watchPosition fix is 0–10 s old, and
  // at line speed that lag is hundreds of metres against a ~5 m accuracy.
  const sample = {
    v: WIRE_VERSION,
    type: "gps",
    id,
    t: pendingGpsT ?? Date.now(),
    gps: pendingGps,
  };
  pendingGps = null;
  pendingGpsT = null;
  gpsCount += 1;
  gpsCountEl.textContent = String(gpsCount);
  gpsEl.textContent = JSON.stringify(sample.gps, null, 2);
  sendSample(sample);
}

// Announce the device once, at the start of a recording session, so the recorder
// can populate the `device` table other tables join to.
function emitStartSession() {
  sendSample({
    v: WIRE_VERSION,
    type: "start_session",
    id,
    t: Date.now(),
    device: deviceInfo(),
  });
}

function sampleTick() {
  emitAccelSample();
  emitGpsSample();
}

// Websocket delivery. Samples go into an outbox persisted to localStorage and flushed
// whenever the socket is open; a dropped connection (train dead zone) triggers
// reconnect with backoff and re-flush. The server acks each delivered sample, and a
// sample is removed from the outbox only once acked — so a page reload or a mid-flush
// disconnect re-sends the un-acked tail rather than losing samples that looked sent.
// The recorder dedups on (device_id, t), so a re-sent duplicate is harmless.
const WS_URL = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`;
const MAX_OUTBOX = 5000;
const INITIAL_RECONNECT_MS = 1000;
const MAX_RECONNECT_MS = 30000;
const OUTBOX_KEY = "lookout_outbox";

let ws = null;
let outbox = loadOutbox();
// Count of outbox entries at the front that have been sent and are awaiting an ack.
// Reset to 0 on (re)connect so anything unacked from a prior connection is re-sent.
let inFlight = 0;
let reconnectMs = INITIAL_RECONNECT_MS;

function loadOutbox() {
  try {
    return JSON.parse(localStorage.getItem(OUTBOX_KEY)) ?? [];
  } catch {
    return [];
  }
}

function persistOutbox() {
  try {
    localStorage.setItem(OUTBOX_KEY, JSON.stringify(outbox));
  } catch {
    // Quota exceeded or storage unavailable (private mode): keep capturing in-memory
    // rather than letting a persistence failure break the recording.
  }
}

// Connect if there's no live socket. Idempotent so both start() and a startup with a
// persisted outbox can call it without opening a second connection.
function ensureWs() {
  if (!ws || ws.readyState === WebSocket.CLOSED) connectWs();
}

function connectWs() {
  ws = new WebSocket(WS_URL);
  ws.addEventListener("open", () => {
    reconnectMs = INITIAL_RECONNECT_MS;
    setStatus("gathering — connected");
    inFlight = 0;
    flushOutbox();
  });
  ws.addEventListener("message", onAck);
  ws.addEventListener("close", () => {
    setStatus("gathering — reconnecting…");
    setTimeout(connectWs, reconnectMs);
    reconnectMs = Math.min(reconnectMs * 2, MAX_RECONNECT_MS);
  });
  // A socket error is followed by a close event, so let close drive the reconnect.
  ws.addEventListener("error", () => ws.close());
}

// The server sends one ack per delivered sample, in order over a single socket, so
// each ack retires the oldest in-flight sample.
function onAck() {
  if (!outbox.length) return;
  outbox.shift();
  inFlight = Math.max(0, inFlight - 1);
  persistOutbox();
}

function flushOutbox() {
  while (inFlight < outbox.length && ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(outbox[inFlight]));
    inFlight += 1;
  }
}

function sendSample(sample) {
  outbox.push(sample);
  if (outbox.length > MAX_OUTBOX) {
    const dropped = outbox.length - MAX_OUTBOX;
    outbox.splice(0, dropped);
    inFlight = Math.max(0, inFlight - dropped);
  }
  persistOutbox();
  flushOutbox();
}

// A screen wake lock keeps iOS from auto-locking (which would suspend the page and
// stop capture). It's released automatically whenever the page hides, so it's
// re-acquired on visibilitychange → visible. It can't survive the power button, and
// Low Power Mode refuses the request — surfaced in the UI so a failure is visible on
// the train.
let wakeLock = null;

async function acquireWakeLock() {
  if (!("wakeLock" in navigator)) {
    wakeLockEl.textContent = "unsupported";
    return;
  }
  try {
    wakeLock = await navigator.wakeLock.request("screen");
    wakeLockEl.textContent = "held";
    wakeLock.addEventListener("release", () => {
      wakeLockEl.textContent = "released";
    });
  } catch (err) {
    // Low Power Mode / a power-button lock refuse the request; capture continues.
    wakeLock = null;
    wakeLockEl.textContent = `refused: ${err.name}`;
  }
}

function onVisible() {
  if (document.visibilityState === "hidden") {
    persistOutbox();
  } else if (recording) {
    acquireWakeLock();
    ensureWs();
  }
}

document.addEventListener("visibilitychange", onVisible);
// pagehide fires on iOS where beforeunload/unload don't; persist the latest outbox.
window.addEventListener("pagehide", persistOutbox);

async function start() {
  startBtn.disabled = true;
  recording = true;

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

  await acquireWakeLock();
  ensureWs();
  emitStartSession();
  setInterval(sampleTick, SAMPLE_INTERVAL_MS);
  setStatus("gathering");
}

startBtn.addEventListener("click", start);

// A persisted outbox from a previous session (a reload mid-trip) still needs
// delivering, so connect and re-flush on startup even before the user hits start.
if (outbox.length) ensureWs();

// Show the server build's git hash, so a running deploy can be matched to source.
fetch("/version")
  .then((r) => r.text())
  .then((v) => {
    el("version").textContent = v;
  })
  .catch(() => {
    el("version").textContent = "unknown";
  });
