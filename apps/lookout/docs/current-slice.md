# Current Slice: Crow-flies predictor on the M5 device and in rerun

## Target

Two halves. One turns what the M5 spikes established
(`spikes/m5/spike7-battery-and-trend`) into production code. The other makes rerun show what
the predictor does.

What exists at the end:

- A predictor in a Crux core, its centre a state machine.
- An M5 build that drives that core from the device's own GPS and predicts the water
  crossings ahead.
- A rerun runner that replays a named session from silver as GPS samples through the same
  core. It draws the predictions against the crossings that session actually made, so we
  watch prediction and water diverge as the run plays out.
- No `visualise` or `spikes/m5` directory.

## What a prediction is

For each crossing within a radius of the current fix: the straight-line distance to it, and
the time we reach it at the current speed. Crow-flies: the track's real geometry plays no
part. A curve or a river bend puts a crossing nearer, and sooner, than the rails do. That is
the baseline the evaluation slice measures against, not the final answer.

## Sketch

### The core

A Crux core built to sit in a different shell each time: the M5 device, the rerun runner,
later an app or a website.

The M5 spikes let events carry raw GNSS strings from the attached receiver. That is too
low-level here. The core instead consumes a **normalised GPS sample** carrying what a
predictor can use. Required: timestamp, latitude, longitude. Optional: altitude, speed,
heading, accuracy, satellite count, and HDOP. A parser converts GNSS strings into that form
for the device. The rerun runner needs no parser — silver `session_sample` already holds
those columns, and a session replays as samples directly.

### The measure

Everything shared is **generic over the float it measures in**, behind one named bound. The
ESP32's FPU is single precision, so `f64` there is emulated in software. A scan of thousands
of crossings against every fix cannot afford that. Off the device `f64` costs nothing and is
what the store already holds. Fixing either type would be unusable on the other platform.

Degrees enter as `f64` whatever the measure, since that is what every source hands over, and
convert once, where they are checked.

The measure matters in one place beyond the scan: deriving a speed from two fixes. `f32`
resolves latitude to about 0.42m, so each fix carries that much error and so does the step
between two of them. A train covers 30m a second, which puts the error near 1%. Walking pace
covers 1.4m, which puts it near 30%. `f32` holds at the speeds this is for, and the device
reports a speed in RMC anyway.

### The state machine

Inside the core, a state machine with a narrower interface. It:

- receives events, each either a GPS sample or a clock advance to a timestamp (a sample
  carries its own timestamp, so it advances the clock too)
- transitions on each event
- answers which crossings it predicts we pass, and when

A minimal generic `trait` captures that interface. A struct implementing it exposes extras
through a second trait: which crossings close, hold, or recede. The panel can then say more
than the prediction alone, and the first trait stays small.

### Layout

Add a `platform` sub-directory under `crates`, holding `m5plus` and `rerun-py`. `m5plus` needs
its own workspace, since it cannot compile as part of the main one. `rerun-py` compiles in the
main workspace.

**The rerun runner is python.** The rerun python SDK carries more of the blueprint API than
the Rust one, and the blueprint is the point of this half of the slice.

**Crux has no python shell.** Its type generation emits Swift, Kotlin/Java, and TypeScript,
and `crux_core`'s `type_generation` module offers nothing else. Its FFI bindings cover Apple,
Android, and WASM. So we write the python binding ourselves.

Write it with pyo3, following `medallion-py`: a `cdylib` with its own `pyproject.toml` and
python tests, built by maturin. The runner's own python lives in the same uv project. Python
then holds the predictor as an object and calls it, with nothing serialised between.

The crux-native alternative, recorded rather than taken: drive the `Bridge`, crux's bincode
`process_event(bytes) -> bytes` boundary, and generate the python types from the same
`serde-reflection` registry crux's typegen builds. `serde-generate` has a python3 backend and
ships `serde` and `bincode` runtimes for it. It buys one thing: python meets the exact byte
interface the device shell does. It costs bincode on both sides, a codegen step, and a
vendored python runtime. Take it once the runner needs to test that boundary rather than the
predictor.

## Open questions

- ~~Whether the state machine's event type is the core's event type or its own.~~ Its own.
  `predict::Event` is `Sampled(Sample)` or `Elapsed(DateTime<Utc>)`, and nothing in the state
  machine knows crux exists. The core's event is a separate type it converts from. That is
  also what lets the python side drive the state machine with no core around it.
- Whether the python extension exposes the state machine directly or the full Crux core. The
  state machine is the smaller surface, and the core is what the device runs.

## Tasks

### 1. The normalised sample

- [x] Define the sample type in a core crate, with newtypes for the coordinates as spike 7
      already has, and `Option` for every field a receiver leaves out. The crate is
      `crates/predictor`, and it holds the state machine and the core too. **Done without
      the newtypes**: a position is a `geo_types::Point`, since georust already has the type
      and spike 7's `Latitude`/`Longitude` were ours to maintain for nothing. The range check
      they carried stays, as the function that builds a position.
- [x] Put spike 7's NMEA accumulation behind a parser that emits samples, keeping its
      captured-sentence tests — the spliced sentence, the bad checksum, and the stationary
      RMC with no course. A sample needs a date, which only RMC carries, so a GGA before the
      first RMC produces nothing. A sentence adding nothing to what is known also produces
      nothing, which keeps a dozen sentences a second from becoming a dozen samples.
- [x] Give it a construction path from `session_sample`'s columns, so the python side builds
      samples from the store without going through NMEA.

### 2. The predictor state machine

- [x] Define both traits: the minimal interface, and the closing/holding/receding extras.
      `Predict` is `observe` and `predictions`. `Trending` answers the trend of one crossing.
      A trend is the distance changing, not the crossing being passed: one we have already
      gone by keeps closing until we are further from it than we started. It has a 10m band,
      since a fix wanders by metres when the geometry is poor.
- [x] Stub it returning no predictions, write the tests against the crow-flies definition
      above, then implement — keeping it compiling at each step. A prediction carries the
      arrival as an instant rather than a countdown, so it stays true while the clock advances
      between fixes. `observe` returns a `Result`: an event dated before the clock is refused
      and changes nothing, rather than being absorbed silently. An event at the same instant
      is accepted, since one fix reaches the predictor as several sentences of one epoch.
- [x] Decide and test what it answers when speed is unknown or zero. Zero, whether reported or
      implied by two fixes in the same place: a distance and no time, because we never arrive.
      Unknown: the speed the step from the previous fix implies, since a phone's geolocation
      routinely reports none. Unknown with no previous fix, which is every session's first
      sample: a distance and no time.

### 3. The Crux core

**Leave BLE off.** Nothing in this slice needs it: the panel is the output, and flash carries
the crossings. The reboot issue that pinned crux to `=0.16.2` appears only with NimBLE running
(see [device.md](device.md)), so leaving BLE out takes the version question off this slice
entirely.

- [ ] Wrap the state machine in a core carrying spike 7's panel view model, extended with
      the predicted times: clock, fix, quality, battery, nearest, within.
- [ ] Keep the crossings carried in flash and the battery judgement in the core, both as
      spike 7 has them.
- [ ] Build against the current `crux_core` rather than the pinned `=0.16.2`.

### 4. `crates/platform/m5plus`

Write this shell fresh from an esp-idf project template rather than lifting spike 7's. The
spikes grew one addition at a time and carry that shape, which we don't want to inherit here.
Build instead from the facts they established, in [device.md](device.md): power hold on
GPIO4, panel offset, GNSS RX pin, stack sizing, and UART ring buffer.

This is where `predictor` first compiles for Xtensa. Nothing before it builds for the device,
so a dependency that cannot cross surfaces here. `geo-types` is the one that has not already
run on the board.

- [ ] Generate the project from the esp-idf template into its own workspace, and get it
      booting with the power hold set.
- [ ] Reach for the higher-level M5 crates **first**, for the battery and for anything else
      they cover. Spike 7 read the ADC by hand only because `m5unified` initialises the
      display alongside power. Weigh that again now we need the display too.
- [ ] Drive the panel and read the GNSS receiver over UART, feeding samples to the core.
- [ ] Flash it and confirm on hardware.

### 5. `crates/platform/rerun-py`

- [ ] Expose the predictor as a python extension module, following `medallion-py`: a pyo3
      `cdylib`, maturin in `pyproject.toml`, and the tests written in python. A rust test
      binary for an extension module has no interpreter to run in.
- [ ] Read a named session's samples from silver in python, with DuckDB over the store as
      `visualise` does today, and feed them through the extension in `t` order.
- [ ] Log the track, each prediction as it is made, and its error against the crossing when
      that crossing arrives.
- [ ] Log silver `session_crossing` as the ground truth to compare against.
- [ ] Give it a blueprint: a map of the session and the crossings, and a timeline of
      predicted times against actual ones.

### 6. Delete what is replaced

- [ ] Delete `visualise/` and its `just visualise` recipe. The bronze GPS and accelerometer
      views and the moving `train_segment` dots go with it. The rerun runner draws the
      predictor and nothing else.
- [ ] Delete `spikes/m5/`, after checking [device.md](device.md) carries every board fact
      worth keeping.
