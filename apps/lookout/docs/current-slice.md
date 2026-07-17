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
* Dry run before the journey: a walk around the block exercises wake lock, outbox persistence across a reload, reconnect with cellular off, and confirms `speed`/`heading` are non-null on the actual device. One device, one browser — no matrix. These failure modes are all silent.

### Tasks

Ordered: the message/session-metadata refactor **first** (it reshapes the wire model
everything else builds on), then capture survival before data quality (data that never
arrives can't be improved). Frontend work is in `crates/server/static/app.js`; the wire
model is `crates/shared/src/lib.rs`; per-sensor tables are
`crates/recorder/src/store.rs`; rerun views are `visualise/main.py`.

#### Refactor messages + introduce session metadata

Before we expand the messages and what they mean, I'd like to first expand how we manage them:
* each json message has a version field which is a simple number; if this is missing, then it is `0`
      * we are introducing version 1 (which should be recorded explicitly)
      * we should be able to understand version 0 messages when re-interpreting from raw
* version 0 consists of these messages:
      * RecordGPS
      * RecordAcceleration
* version 1, which we are introducing, should consist of:
      * RecordGPS
            * contains new fields
      * RecordAcceleration
            * contains new fields
      * StartSession
            * a totally new message which should logged by a client whenever it starts a new session e.g. by the button to start recording being pressed
            * this should have a timestamp, device id etc, but should also contain metadata like os platform, device type etc; enough to tell if this an iPad/iPhone/Laptop
            * when interpreted this should map to a new table called `device`, with device_id as a primary key; this can be used to join to from other tables to get device metadata

This probably be represented as a two-level enum e.g.
* Version0
      * Contains enum for RecordGPS,RecordAcceleration choices
* Version1
      * Contains enum for StartSession,RecordGPS,RecordAcceleration choices

##### Tasks

Do this refactor **first** — the data-quality field additions below then land as the
"new fields" on the v1 `RecordGPS` / `RecordAcceleration` variants, rather than being
retrofitted onto the flat `Sample`. Keep it compiling at each step (stub → failing test
→ implement).

- [x] **Model the versioned message enum in `shared`.** Replace the flat `Sample` with a
      two-level enum: outer `Version0` / `Version1`, each holding an inner message enum
      (v0: `RecordGps` / `RecordAcceleration`; v1: adds `StartSession`). Carry the `v`
      number on the wire, defaulting to `0` when absent (`#[serde(default)]`). Pick the
      serde tagging (e.g. `v` + a message `type` tag) and document the wire shape in the
      module doc. Keep v1 `RecordGps`/`RecordAcceleration` field-identical to v0 for now
      — the new fields come from the data-quality tasks.
- [x] **Roundtrip tests for both versions.** Assert: a payload with no `v` decodes as
      Version0; an explicit `v:1` decodes as Version1; each version's message variants
      roundtrip; the exact v0 wire shapes currently stored in `raw` still parse
      (re-interpretation from raw must not break). Use captured real payloads.
- [x] **`StartSession` message + client metadata.** Define `StartSession` with
      timestamp, device id, and enough metadata to distinguish iPad / iPhone / Laptop
      (os platform, device type). In `app.js`, send a v1 `StartSession` when recording
      starts (the start button), and stamp `v:1` + the message `type` on every emitted
      GPS/accel sample. Derive the metadata from what Safari exposes (`navigator`
      platform / UA-CH); no non-Safari fallbacks.
- [x] **`device` table in `store.rs`.** On a `StartSession`, upsert a row into a new
      `device` table keyed on `device_id` (primary key), holding the session metadata,
      so other tables can join to it. Route messages by version/type when deriving
      per-sensor rows; a v0 payload still populates `accel` / `gps`. Update `store.rs`
      tests.
- [x] **Server ingest still validates + queues** the new shape. `handle_sample` parses
      into the enum (rejecting genuinely malformed payloads) and queues raw verbatim as
      today — confirm both v0 and v1 payloads pass. Update `crates/server` tests.

#### Capture survival

- [x] **Wake lock.** Acquire `navigator.wakeLock` in `start()`; re-acquire on
      `visibilitychange` → visible (it's auto-released on hide). Tolerate rejection (Low
      Power Mode / power button) without breaking capture.
- [x] **Surface wake-lock state in the status UI** so a failure is visible on the train
      (held / released / refused). Add the field to `index.html` + `app.js`.
- [x] **Persist the outbox to `localStorage`.** Write on every `sendSample` enqueue; load
      and re-flush on startup. Use `pagehide` / `visibilitychange` → hidden for any
      flush-on-exit (not `beforeunload`/`unload` — unreliable on iOS).
- [x] **Server acks before outbox removal.** Server sends an ack per received sample;
      `flushOutbox` shifts a sample off only once acked, so a drop mid-flush doesn't lose
      samples that looked sent. (Do after persistence — needs `crates/server/src/lib.rs`
      `handle_socket` to reply.)

#### Data quality (wire model + frontend)

- [x] **Extend `shared::Gps`** with `speed: Option<f64>` and `heading: Option<f64>`
      (both nullable; a null carries meaning). Update roundtrip tests.
- [x] **Redefine `shared::Accel`** as aggregate `{ rms, peak, n }`, **keeping** a raw
      instantaneous x/y/z reading alongside (decided: keep it — costs nothing, preserves
      a tilt view). Update tests.
- [x] **Capture `coords.speed` / `coords.heading`** in `onPosition`; include in
      `emitGpsSample`. Store nulls, don't drop them.
- [x] **Use `position.timestamp` as `t`** for the gps sample, not `Date.now()` (a fix is
      0–10 s old; at 160 km/h that's hundreds of metres). Same epoch-millis basis.
- [x] **Aggregate accel in `onMotion`** instead of overwriting: accumulate
      `event.acceleration` (gravity-removed) across the window, emit `{ rms, peak, n }`
      on the existing tick. `onMotion` loses its `accelerationIncludingGravity` branch
      (iOS-only, no fallback).

#### Persistence + server

- [x] **Add the new columns to the per-sensor tables** in `store.rs`: `speed`, `heading`
      on `gps`; `rms`, `peak`, `n` on `accel`. No migration — raw table is unchanged and
      per-sensor tables rebuild from raw. Update `store.rs` tests.
- [x] **Server-side `received_at`** stamped on ingest (device clocks drift; `t` is
      device-stamped). Lands in a **separate column** (decided) — keeps the
      raw-verbatim/md5 idempotency contract intact rather than mutating the payload.
      Note: stamped in the **server** at websocket handling time (not at recorder
      archive time — the recorder can drain much later). It rides through the queue in
      a `RawSample` envelope (`{ received_at, payload }`) beside the verbatim payload,
      and the recorder writes it to the `received_at` column.

#### Views + dry run

- [x] **Update rerun views** in `visualise/main.py`. Add the new columns to the
      `fetch_accel` / `fetch_gps` queries first, then:
      * colour the `GeoLineStrings` track by `speed` via `colors=` — the payoff for
        capturing it, and the view this whole thing has been pointing at.
      * add `rms` as ride roughness, `peak` as jolts/pointwork. RMS supersedes the `|a|`
        magnitude series: magnitude only says the device was disturbed, RMS is actual
        ride quality.
      * add `n` as its own series — capture health, showing windows where the page was
        suspended.
      * the retained raw x/y/z is device tilt, not train acceleration. Label it that way
        or leave it out of the default blueprint.
- [-] **Dry run** before the journey (walk round the block): wake lock holds, outbox
      survives a reload, reconnect works with cellular off, `speed`/`heading` non-null on
      the real device. One device, one browser — these failure modes are all silent.
      * decided to just risk it on the day
