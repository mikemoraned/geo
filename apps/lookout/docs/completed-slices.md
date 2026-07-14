# Completed Slices

Append-only history of finished slices. Never edit existing entries; `/complete-slice`
adds a new condensed summary here when a slice is done.

## bootstrap getting sensor data and saving it in rerun.io format

Aimed to stand up a minimal localhost-only pipeline: a Rust web server serving a page
that reads laptop accelerometer data, streams it over a websocket, and persists it in
rerun.io format. **Abandoned part-way on a learned constraint**, but the transport half
was built and verified.

- Scaffolded a bobby-inspired `crates/` cargo workspace with a new axum `server` crate
  serving static assets on localhost, plus a `/ws` websocket endpoint that receives and
  logs JSON accel samples. Added a vanilla `index.html` + `app.js` front-end doing the
  `DeviceMotionEvent` permission flow, listener, and websocket send.
- Verified the browser→server websocket transport works end to end (socket connects,
  server logs samples). The rerun persistence half and the planned crux/ports-and-adapters
  refactor were not reached.
- **Key constraint discovered:** the dev machine (Apple Silicon M3 MacBook Air) has no
  accelerometer — Apple Silicon dropped the Sudden Motion Sensor, so `DeviceMotionEvent`
  never fires on the laptop. A localhost-only, laptop-only setup can validate transport
  and persistence but cannot source real motion data.
- **Implication:** future slices needing real motion must use an external source —
  AirPods (`CMHeadphoneMotionManager`), an IMU game controller (native HID), or an
  iPhone/iPad over HTTPS (needs a cert or tunnel, the LAN work this slice deferred).
  Recorded in `target.md` under Learned Constraints.

