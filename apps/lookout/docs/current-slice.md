# Current Slice: Crow-flies predictor deployed to M5 device and rerun simulation

## Target

Two halves. One turns what the M5 spikes established
(`spikes/m5/spike7-battery-and-trend`) into production code. The other makes rerun show what
the predictor is doing.

What exists at the end:

- A predictor in a Crux core, its centre a state machine.
- An M5 build that drives that core from the device's own GPS and predicts the water
  crossings ahead.
- A rerun simulation that re-drives a named session from silver as GPS samples through the
  same core, captures its predictions, and draws them against the crossings that session
  actually made — so we can watch the predictions and the water diverge as the run plays out.
- No `visualise` directory and no `spikes/m5` directory.

## What a prediction is

For each crossing within a radius of the current fix: how far away it is in a straight line,
and the time we reach it at the current speed. Crow-flies — the track's real geometry plays no
part, so a curve or a river bend puts a crossing nearer, and sooner, than the rails do. That
is the baseline the evaluation slice measures against, not the final answer.

## Sketch

### The core

A Crux core built to sit in a different shell each time: the M5 device, a rerun runner, or
later an app or a website.

The M5 spikes let events carry raw GNSS strings from the attached receiver. That is too
low-level here. The core instead consumes a **normalised GPS sample** carrying what a
predictor can use: timestamp, latitude, longitude, and the optional altitude, speed, heading,
accuracy, satellite count, and HDOP. A parser converts GNSS strings into that form for the
device. The rerun runner needs no parser — silver `session_sample` already holds those
columns, and a session replays as samples directly.

### The state machine

Inside the core, a state machine with a narrower interface. It:

- receives events, each either a GPS sample or a clock advance to a timestamp (a sample
  carries its own timestamp, so it advances the clock too)
- transitions on each event
- answers which crossings it predicts we pass, and when

A minimal generic `trait` captures that interface. A struct implementing it exposes extras
through a second trait — which crossings close, hold, or recede — so the panel can say more
than the prediction alone while the first trait stays small.

### Layout

Add a `platform` sub-directory under `crates`, holding `m5plus` and `rerun-py`. `m5plus` needs
its own workspace, since it cannot compile as part of the main one. `rerun-py` compiles in the
main workspace.

**The rerun runner is python.** The rerun python SDK carries more of the blueprint API than
the Rust one, and the blueprint is the whole point of this half of the slice.

**Crux has no python shell.** Its type generation emits Swift, Kotlin/Java, and TypeScript —
`crux_core`'s `type_generation` module offers those and nothing else — and its FFI bindings
are generated for Apple, Android, and WASM. So we write the python binding ourselves.

Write it with pyo3, following `medallion-py`: a `cdylib` with its own `pyproject.toml` and
python tests, built by maturin. The simulation's own python lives in the same uv project.
Python then holds the predictor as an object and calls it, with nothing serialised between.

The crux-native alternative, recorded rather than taken: drive the `Bridge` — crux's bincode
`process_event(bytes) -> bytes` boundary — and generate the python types from the same
`serde-reflection` registry crux's typegen builds, since `serde-generate` has a python3
backend and ships `serde` and `bincode` runtimes for it. It buys one thing: python meets the
exact byte interface the device shell does. It costs bincode on both sides, a codegen step,
and a vendored python runtime. Take it once the simulation needs to test that boundary rather
than the predictor.

## Open questions

- Whether the state machine's event type is the core's event type or its own.
- Whether the python extension exposes the state machine directly or the full Crux core. The
  state machine is the smaller surface; the core is what the device runs.

## Tasks

### 1. The normalised sample

- [x] Define the sample type in a core crate, with newtypes for the coordinates as spike 7
      already has, and `Option` for every field a receiver leaves out. The crate is
      `crates/predictor`, and it will hold the state machine and the core too. **Done
      without the newtypes**: a position is `geo_types::Point<f64>`, since georust already
      has the type and spike 7's `Latitude`/`Longitude` were ours to maintain for nothing.
      The range check they carried stays, as the function that builds a position.
- [x] Put spike 7's NMEA accumulation behind a parser that emits samples, keeping its
      captured-sentence tests — the spliced sentence, the bad checksum, and the stationary
      RMC with no course. A sample needs a date, which only RMC carries, so a GGA before the
      first RMC produces nothing; and a sentence adding nothing to what is known produces
      nothing, which is what keeps a dozen sentences a second from becoming a dozen samples.
- [x] Give it a construction path from `session_sample`'s columns, so the python side builds
      samples from the store without going through NMEA.

### 2. The predictor state machine

- [ ] Define both traits: the minimal interface, and the closing/holding/receding extras.
- [ ] Stub it returning no predictions, write the tests against the crow-flies definition
      above, then implement — keeping it compiling at each step.
- [ ] Decide and test what it answers when speed is unknown or zero.

### 3. The Crux core

**Leave BLE off.** Nothing in this slice needs it: the panel is the output, and the crossings
are carried in flash. The reboot that pinned crux to `=0.16.2` only appears with NimBLE
running (see [device.md](device.md)), so leaving BLE out takes the version question off this
slice entirely. It returns whenever BLE does.

- [ ] Wrap the state machine in a core carrying spike 7's panel view model — clock, fix,
      quality, battery, nearest, within — extended with the predicted times.
- [ ] Keep the crossings carried in flash and the battery judgement in the core, both as
      spike 7 has them.
- [ ] Build against the current `crux_core` rather than the pinned `=0.16.2`.

### 4. `crates/platform/m5plus`

Write this shell fresh from an esp-idf project template rather than lifting spike 7's. The
spikes grew one addition at a time and carry that shape; the facts they established are in
[device.md](device.md) and are what to build from — power hold on GPIO4, panel offset, GNSS
RX pin, stack sizing, and UART ring buffer.

This is where `predictor` is first compiled for Xtensa. Nothing before it builds for the
device, so a dependency that cannot cross surfaces here. `geo-types` is the one that has not
already run on the board.

- [ ] Generate the project from the esp-idf template into its own workspace, and get it
      booting with the power hold set.
- [ ] Reach for the higher-level M5 crates **first**, for the battery and for anything else
      they cover. Spike 7 read the ADC by hand only because `m5unified` initialises the
      display alongside power; weigh that again now the display is needed too.
- [ ] Drive the panel and read the GNSS receiver over UART, feeding samples to the core.
- [ ] Flash it and confirm on hardware.

### 5. `crates/platform/rerun-py`

- [ ] Expose the predictor as a python extension module, following `medallion-py`: a pyo3
      `cdylib`, maturin in `pyproject.toml`, and the tests written in python, since a rust
      test binary for an extension module has no interpreter to run in.
- [ ] Read a named session's samples from silver in python — DuckDB over the store, as
      `visualise` does today — and feed them through the extension in `t` order.
- [ ] Log the track, each prediction as it is made, and its error against the crossing when
      that crossing arrives.
- [ ] Log silver `session_crossing` as the ground truth to compare against.
- [ ] Give it a blueprint: a map of the session and the crossings, and a timeline of
      predicted times against actual ones.

### 6. Delete what is replaced

- [ ] Delete `visualise/` and its `just visualise` recipe. The bronze GPS and accelerometer
      views and the moving `train_segment` dots go with it; the rerun platform draws the
      predictor and nothing else.
- [ ] Delete `spikes/m5/`, after checking [device.md](device.md) carries every board fact
      worth keeping.
