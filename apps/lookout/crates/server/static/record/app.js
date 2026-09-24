const el = (id) => document.getElementById(id);
const statusEl = el("status");
const wakeLockEl = el("wakelock");
const idEl = el("id");
const accelEl = el("accel");
const accelCountEl = el("accel-count");
const gpsEl = el("gps");
const gpsCountEl = el("gps-count");
const startBtn = el("start");

let recording = false;

const DEVICE_ID_COOKIE = "lookout_device_id";

const SAMPLE_INTERVAL_MS = 10000;

let accelCount = 0;
let gpsCount = 0;

let accelSumSq = 0;
let accelPeak = 0;
let accelN = 0;
let lastAccel = null;

let pendingGps = null;
let pendingGpsTakenAt = null;

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

const WIRE_VERSION = 1;

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
  pendingGps = {
    lat: c.latitude,
    lon: c.longitude,
    alt: c.altitude,
    acc: c.accuracy,
    speed: c.speed,
    heading: c.heading,
  };
  pendingGpsTakenAt = position.timestamp;
  takeFirstGpsSample();
}

function onPositionError(err) {
  if (err.code === err.PERMISSION_DENIED) {
    gpsEl.textContent = "permission denied";
  } else if (gpsCount === 0) {
    gpsEl.textContent = "waiting for a gps fix…";
  }
}

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
  const sample = {
    v: WIRE_VERSION,
    type: "gps",
    id,
    t: pendingGpsTakenAt ?? Date.now(),
    gps: pendingGps,
  };
  pendingGps = null;
  pendingGpsTakenAt = null;
  gpsCount += 1;
  gpsCountEl.textContent = String(gpsCount);
  gpsEl.textContent = JSON.stringify(sample.gps, null, 2);
  sendSample(sample);
}

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

const WS_URL = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`;
const MAX_OUTBOX = 5000;
const INITIAL_RECONNECT_MS = 1000;
const MAX_RECONNECT_MS = 30000;
const OUTBOX_KEY = "lookout_outbox";

let ws = null;
let outbox = loadOutbox();
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
  }
}

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
  ws.addEventListener("error", () => ws.close());
}

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
window.addEventListener("pagehide", persistOutbox);

async function start() {
  startBtn.disabled = true;
  recording = true;

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

if (outbox.length) ensureWs();

