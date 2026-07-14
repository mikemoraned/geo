const statusEl = document.getElementById("status");
const countEl = document.getElementById("count");
const sampleEl = document.getElementById("sample");
const startBtn = document.getElementById("start");

let ws = null;
let count = 0;

function setStatus(text) {
  statusEl.textContent = text;
}

function openSocket() {
  const url = `ws://${location.host}/ws`;
  ws = new WebSocket(url);
  ws.addEventListener("open", () => setStatus("connected"));
  ws.addEventListener("close", () => setStatus("disconnected"));
  ws.addEventListener("error", () => setStatus("socket error"));
}

function onMotion(event) {
  // includeGravity acceleration is present on both Safari and Chrome.
  const a = event.accelerationIncludingGravity || event.acceleration || {};
  const sample = {
    t: Date.now(),
    x: a.x ?? null,
    y: a.y ?? null,
    z: a.z ?? null,
  };
  sampleEl.textContent = JSON.stringify(sample, null, 2);
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(sample));
    count += 1;
    countEl.textContent = String(count);
  }
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
      setStatus(`permission error: ${err}`);
      startBtn.disabled = false;
      return;
    }
  }

  openSocket();
  window.addEventListener("devicemotion", onMotion);
  setStatus("listening");
}

startBtn.addEventListener("click", start);
