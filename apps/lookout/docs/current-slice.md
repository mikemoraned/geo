# Current Slice: improve train-based recording accuracy / reliability

### Target

We want to get a reliable stream of data recorded from a device that captures as much info as possible (doesn't lose it) and with as much quality as possible.

### Info / Suggestions

**Scope: iOS/macOS only — Android is explicitly not supported.** All current devices are personal iOS/macOS ones, so the only capture target is Safari, and in practice the only *sensor* target is the iPhone/iPad (the M3 laptop has no accelerometer, and macOS geolocation is wifi-derived — macOS is a dev/test target for the page, not a data source). No feature detection or fallback paths for non-Safari browsers.

Two groups: capture survival (whether data arrives at all), and data quality (whether it means anything). Roughly priority-ordered.

Background for most of the quality items: at 0.1 Hz instantaneous sampling, the accelerometer measures gravity, not the train. Gravity is 9.81 m/s²; train acceleration is ~0.5–1.3 m/s², lateral ~1 m/s². Ride vibration is 1–50 Hz, so at 0.1 Hz it aliases into noise. `|a|` reads 9.81 except when the device is handled.

#### Capture survival

* **iOS suspends the page.** Screen lock or backgrounding stops `setInterval` and `devicemotion`, so `sampleTick` never fires. A phone in a pocket records nothing. No true background execution is available to a web app on iOS.
* **`navigator.wakeLock`**, acquired in `start()` and re-acquired on `visibilitychange` → visible. The lock is released automatically whenever the page hides, so without re-acquire it's lost for the rest of the trip. Safari 16.4+.
* **Wake lock only prevents auto-lock.** It doesn't survive the power button, and Low Power Mode refuses the request. So there's an operating procedure too: phone flat, plugged in, screen dimmed, Low Power Mode off. Showing wake-lock state in the status UI makes a failure visible while still on the train.
* **Persist the outbox to `localStorage`.** Currently in-memory, so a reload or tab reap drops it. At 0.1 Hz just write on every enqueue — 5000 × ~150 bytes is well under the ~5 MB quota. Load on startup.
* **Use `pagehide` / `visibilitychange` → hidden** for any timing-based persistence; `beforeunload` and `unload` don't fire reliably on iOS.
* **`ws.send()` only queues into the socket buffer**, but `flushOutbox` shifts the sample off immediately, so a drop mid-flush loses samples that looked sent. Server acks before removal would fix it. Do after persistence.

#### Data quality

* **Capture `coords.speed`.** Doppler-derived (~0.1 m/s), not differenced positions. Differencing it over 10 s gives longitudinal acceleration at ~0.014 m/s² error against a 0.5–1.3 m/s² signal — the quantity the accelerometer can't provide.
* **It can't be reconstructed later.** Differencing stored positions gives 10 s-averaged speed with error from position accuracy, and the averaging flattens ~60 s events like braking into a station. In cuttings where accuracy drops to tens of metres it's unusable; Doppler speed isn't.
* **Capture `coords.heading`** — course over ground, so the train's direction. With speed, gives lateral acceleration via `v × dψ/dt`.
* **Both are nullable; store the nulls.** `heading` is null when stationary; a null speed at a platform is information.
* **Use `position.timestamp`, not `Date.now()`.** `pendingGps` holds a fix 0–10 s old. At 160 km/h a 5 s lag is ~220 m error (440 m worst case), against a recorded `acc` of ~5 m. Same epoch-millis basis. Accel is unaffected — `devicemotion` fires ~60 Hz.
* **Aggregate accel in `onMotion` instead of overwriting.** ~599 of every 600 readings are currently discarded. Accumulating and emitting on the existing tick changes no rates, wire format, or downstream code, and averaging before the aliasing gives a real signal at 0.1 Hz.
  * Emit `{ rms, peak, n }`: RMS = ride roughness, peak = jolts/pointwork, `n` = confirmation the window was sampled rather than suspended.
  * Use `event.acceleration` (gravity-removed). Its magnitude is orientation-invariant, so device placement stops mattering. iOS-only means no `accelerationIncludingGravity` fallback chain — `onMotion` loses its branch, and the per-axis `Option`s in `shared::Accel` can collapse, since the nullable-per-component shape was a `DeviceMotionEvent` artefact rather than something iOS produces.
  * Open: keep raw instantaneous x/y/z alongside? Costs nothing, preserves a tilt view, but isn't the ride signal.
* **Add a server-side `received_at`** on ingest — device clocks drift, and `t` is device-stamped.

#### Consequences elsewhere

* New fields (`speed`/`heading` on gps, `rms`/`peak`/`n` on accel) need adding to the per-sensor tables in `recorder`. No migration — they land in the raw table regardless, and per-sensor tables rebuild from raw.
* Rerun views change: the `Arrows2D` accel vector shows device tilt, not train acceleration — drop or relabel. Useful instead: `|a| - 9.81` as a disturbance indicator, RMS as roughness, `GeoLineStrings` coloured by Doppler speed.
* Dry run before the journey: a walk around the block exercises wake lock, outbox persistence across a reload, reconnect with cellular off, and confirms `speed`/`heading` are non-null on the actual device. One device, one browser — no matrix. These failure modes are all silent.

### Tasks

Priority-ordered: capture survival before data quality (data that never arrives can't
be improved). Frontend work is in `crates/server/static/app.js`; the wire model is
`crates/shared/src/lib.rs`; per-sensor tables are `crates/recorder/src/store.rs`; rerun
views are `visualise/main.py`.

#### Capture survival

- [ ] **Wake lock.** Acquire `navigator.wakeLock` in `start()`; re-acquire on
      `visibilitychange` → visible (it's auto-released on hide). Tolerate rejection (Low
      Power Mode / power button) without breaking capture.
- [ ] **Surface wake-lock state in the status UI** so a failure is visible on the train
      (held / released / refused). Add the field to `index.html` + `app.js`.
- [ ] **Persist the outbox to `localStorage`.** Write on every `sendSample` enqueue; load
      and re-flush on startup. Use `pagehide` / `visibilitychange` → hidden for any
      flush-on-exit (not `beforeunload`/`unload` — unreliable on iOS).
- [ ] **Server acks before outbox removal.** Server sends an ack per received sample;
      `flushOutbox` shifts a sample off only once acked, so a drop mid-flush doesn't lose
      samples that looked sent. (Do after persistence — needs `crates/server/src/lib.rs`
      `handle_socket` to reply.)

#### Data quality (wire model + frontend)

- [ ] **Extend `shared::Gps`** with `speed: Option<f64>` and `heading: Option<f64>`
      (both nullable; a null carries meaning). Update roundtrip tests.
- [ ] **Redefine `shared::Accel`** as aggregate `{ rms, peak, n }`, **keeping** a raw
      instantaneous x/y/z reading alongside (decided: keep it — costs nothing, preserves
      a tilt view). Update tests.
- [ ] **Capture `coords.speed` / `coords.heading`** in `onPosition`; include in
      `emitGpsSample`. Store nulls, don't drop them.
- [ ] **Use `position.timestamp` as `t`** for the gps sample, not `Date.now()` (a fix is
      0–10 s old; at 160 km/h that's hundreds of metres). Same epoch-millis basis.
- [ ] **Aggregate accel in `onMotion`** instead of overwriting: accumulate
      `event.acceleration` (gravity-removed) across the window, emit `{ rms, peak, n }`
      on the existing tick. `onMotion` loses its `accelerationIncludingGravity` branch
      (iOS-only, no fallback).

#### Persistence + server

- [ ] **Add the new columns to the per-sensor tables** in `store.rs`: `speed`, `heading`
      on `gps`; `rms`, `peak`, `n` on `accel`. No migration — raw table is unchanged and
      per-sensor tables rebuild from raw. Update `store.rs` tests.
- [ ] **Server-side `received_at`** stamped on ingest (device clocks drift; `t` is
      device-stamped). Lands in a **separate column** (decided) — keeps the
      raw-verbatim/md5 idempotency contract intact rather than mutating the payload.

#### Views + dry run

- [ ] **Update rerun views** in `visualise/main.py`: relabel/drop the device-tilt accel
      vector; add `|a| - 9.81` disturbance / RMS roughness; colour `GeoLineStrings` by
      Doppler speed. Add new columns to the `fetch_accel` / `fetch_gps` queries.
- [ ] **Dry run** before the journey (walk round the block): wake lock holds, outbox
      survives a reload, reconnect works with cellular off, `speed`/`heading` non-null on
      the real device. One device, one browser — these failure modes are all silent.
