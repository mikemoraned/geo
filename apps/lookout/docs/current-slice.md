# Current Slice: bootstrap getting sensor data and saving it in https://rerun.io format

### Target

We want to get the most minimal setup of:
* local server running in rust serving a web page
* browser and server both run on the laptop — everything is **localhost only**
* that web page is loaded in Safari (and Chrome) and accesses accelerometer data
* accelerometer data is sent back to the listening rust server over a websocket connection
* rust server saves that data to disk in rerun.io format
* data is loaded in rerun.io app and visualised

### Decisions

* **Crate layout** follows https://github.com/mikemoraned/bobby: a `crates/` workspace
  with a core `shared` crate surrounded by support crates (`web-support`, `test-support`,
  etc.). Copy / be-inspired by that layout.
* **localhost only**: browser and server both run on the laptop. `http://localhost` is a
  secure context, so no HTTPS/cert/LAN setup is needed. Phone/tablet devices are out of
  scope for this slice.
* **Sensor API**: use the `DeviceMotionEvent` API, which works on *both* Safari and
  Chrome — not the linked Generic Sensor `Accelerometer` interface, which Safari does not
  support.
* **Front-end**: minimal vanilla HTML/JS for the spike. TypeScript + SolidJS
  (per target.md) is deferred — it is *not* part of this slice.
* **Transport**: a single websocket carrying accel samples as JSON.
* **Persistence**: the `rerun` Rust SDK writes a `.rrd` file to disk, logging accel as
  three scalar time series (x / y / z) under an entity path (e.g. `/accel/x`). Offline
  file logging, not live streaming; open the `.rrd` in the rerun viewer afterwards.

### Tasks

See target.md for overall guiding advice.

**Spike — get it working at all (no crux, no Solid):**

* [ ] Scaffold the `crates/` cargo workspace + an axum server crate that compiles and
      serves a static "hello" page over HTTP on localhost.
* [ ] Static front-end page (vanilla HTML/JS): subscribe to `DeviceMotionEvent` and open a
      websocket to the server. Confirm it runs on localhost in both Safari and Chrome.
* [ ] Websocket endpoint on the server that receives and deserializes accel samples.
* [ ] Persist received samples via the `rerun` SDK: log x/y/z as scalar time series to a
      `.rrd` file; flush/save on disconnect or shutdown.
* [ ] Verify end-to-end: run the page on localhost, then open the `.rrd` in the rerun
      viewer and see the three curves.

**Refactor — introduce crux and split into ports/adapters:**

* [ ] Introduce a `shared` crux core crate holding the domain (sample model, Event/Model,
      persistence as a capability/effect); move business logic into it.
* [ ] Split the adapters behind ports: websocket/sensor-input adapter and rerun/persistence
      adapter, per the ports-and-adapters pattern, in a bobby-like crate layout.
* [ ] Keep it compiling at every step and re-verify the end-to-end flow is unchanged.
