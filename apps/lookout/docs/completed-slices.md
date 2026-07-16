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

## mike is on a train getting data

Built the full pipeline end to end: an iPhone samples GPS + accelerometer over the
train journey, streams them to a fly.io-hosted server, and the data is drained locally
into SQLite and visualised in rerun. The laptop-has-no-accelerometer constraint was
resolved by making the phone the sensor and serving over fly.io's public HTTPS (secure
context for Geolocation / DeviceMotion, `wss://` websocket — no cert/LAN work needed).

- **Frontend**: vanilla page that persists a `crypto.randomUUID()` device id in a cookie,
  samples `devicemotion` + `geolocation.watchPosition` throttled to a fixed interval, and
  sends timestamped JSON samples over the websocket with a best-effort in-memory outbox
  flushed on reconnect (not persisted, drops oldest on overflow).
- **Transport**: server `LPUSH`es samples onto an upstash redis list; pushing goes through
  a `SampleSink` port (`RedisSink` for prod) so `/ws` is covered by a Docker-free integ
  test. Redis is optional (log-only when unset) but fails loud when configured yet
  unreachable. Added a `/version` endpoint + build-git-hash startup log for debugging.
- **New crates**: `shared` (the `Sample` model), `telemetry` (queue connect/drain, returns
  a lossless `RawSample`), and a `recorder` cli. Deleted the rerun-as-archive plan.
- **Recorder cli**: `view-latest` (non-destructive `LRANGE`) and `drain` (destructive
  `BRPOP`) modes writing to SQLite — a lossless `raw(md5,json)` table plus per-sensor
  `accel`/`gps` tables, all `INSERT OR IGNORE`. Per-sensor tables are a rebuildable
  derivation of raw.
- **Visualise**: a Python `uv` project (rerun-sdk 0.34) converting SQLite → rrd, selecting
  by `--since <Nd>` / `--devices`. Blueprint pairs a map view with per-device accel
  time-series. GPS logged as a static `GeoLineStrings` track plus per-fix `GeoPoints` with
  accuracy radii; accel logged as one `send_columns` entity with a derived `|a|` magnitude
  series (the one orientation-invariant signal in gravity-dominated data).
- **Secrets**: bobby's 1Password pattern — checked-in `deploy/*.env` hold only `op://`
  references; local runs wrap in `op run`, fly deploy pushes resolved values via
  `fly secrets set`. No secret values committed.
- Proved on a real journey: 3 devices, 292 accel + 163 gps samples drained to
  `data/lookout.sqlite`, converted to `.rrd` (`rerun rrd verify` passes), and confirmed
  visible in the rerun viewer.

