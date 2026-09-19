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
- ~~Whether the python extension exposes the state machine directly or the full Crux core.~~
  The state machine. The runner draws predictions, which is what `Predict` and `Trending`
  answer; the core adds a panel view model that nothing off the device reads.

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
the crossings. ~~The reboot issue that pinned crux to `=0.16.2` appears only with NimBLE
running (see [device.md](device.md)), so leaving BLE out takes the version question off this
slice entirely.~~ **Refuted on hardware**: 0.20 double-faults on the first event with no radio
on the board at all. BLE was never the precondition — it only looked like one while 0.19 was
the only broken version tried. The pin is back to `=0.16.2`.

- [x] Wrap the state machine in a core carrying spike 7's panel view model, extended with
      the predicted times: clock, fix, quality, battery, nearest, within. The crate is
      `crates/platform-core`, next to the `crates/platform` shells that drive it. A crossing's line is its distance and a countdown, and **no
      longer its id**: 13 characters do not hold all three, and eight hex digits naming a row
      in a dataset say nothing to someone on a train.
- [x] Keep the crossings carried in flash and the battery judgement in the core, both as
      spike 7 has them. Scanning them where they lie needed a `Crossings` source in
      `predictor`, since `CrowFlies` took a `Vec` — building one would have copied 69 KB of
      flash into RAM, against what [device.md](device.md) records.
- [-] Build against the current `crux_core` rather than the pinned `=0.16.2`. That is 0.20,
      which drops the `Capabilities` associated type and the `caps` argument to `update`.
      **Moot**: 0.20 reboots the device, so the pin stays and the associated type comes back.
      Done and then undone, which is what establishes it — the version question could not be
      settled off the board.

### 4. `crates/platform/m5plus`

Write this shell fresh from an esp-idf project template rather than lifting spike 7's. The
spikes grew one addition at a time and carry that shape, which we don't want to inherit here.
Build instead from the facts they established, in [device.md](device.md): power hold on
GPIO4, panel offset, GNSS RX pin, stack sizing, and UART ring buffer.

This is where `predictor` first compiles for Xtensa. Nothing before it builds for the device,
so a dependency that cannot cross surfaces here. `geo-types` is the one that has not already
run on the board — and it crosses: `predictor` and `platform-core` both build for
`xtensa-esp32-espidf` untouched.

- [x] Generate the project from the esp-idf template into its own workspace, and get it
      booting with the power hold set. The crate is `crates/platform/m5plus`, and it is
      excluded from the app workspace by name: `members = ["crates/*"]` would otherwise try to
      read a manifest in `crates/platform` itself and refuse to load the workspace at all.
      ESP-IDF installs under the crate rather than `~/.espressif`, because the sandbox only
      writes inside `apps/lookout`.
- [x] Reach for the higher-level M5 crates **first**, for the battery and for anything else
      they cover. Spike 7 read the ADC by hand only because `m5unified` initialises the
      display alongside power. Weigh that again now we need the display too. **Weighed and
      declined**: the crate's shim pins ESP-IDF below the 5.5 this board runs, and wants
      vendoring as a C++ component. See [device.md](device.md).
- [x] Drive the panel and read the GNSS receiver over UART, feeding samples to the core.
      **The shell sends no `Tick`**: this board's clock counts from the epoch at boot, with no
      NTP and no RTC, so every tick would be behind the receiver and refused. The panel's clock
      is the predictor's, which a fix advances — one line changed in the core, and `Model::now`
      went with it.
- [x] Flash it and confirm on hardware. Claude cannot open the serial port, so this is
      `just m5plus-flash` run by hand and the boot log read back. Confirmed on a real fix: the
      panel's clock is the receiver's UTC, the position and quality lines read, and the scan
      runs at 1 Hz against the whole set for no measurable stack. **A crossing line has never
      been drawn**, because the carried set is Germany and the fix was in Scotland, so the
      count correctly reads `0 in 5km`. The predictions themselves are covered by the core's
      tests and are what the rerun runner exercises end to end.
- [x] Move the ESP-IDF install to `~/.espressif`, so one copy serves every worktree instead
      of 4.3 GB under each. The shell's `.cargo/config.toml` is on
      `ESP_IDF_TOOLS_INSTALL_DIR = "global"`, and the root `Justfile` grants the sandbox
      read-only access to `~/.espressif`, since safehouse denies by default and refused even to
      read it. Confirmed with the in-crate `.embuild` gone and `esp-idf-sys` rebuilt from
      scratch. Two things learnt on the way: changing the install dir invalidates the cmake
      cache under `target/`, which fails as a source-directory mismatch rather than anything
      naming the setting; and only the first build writes to the install dir, so read-only
      access is enough for every build after it.

### 5. `crates/platform/rerun-py`

- [x] Expose the predictor as a python extension module, following `medallion-py`: a pyo3
      `cdylib`, maturin in `pyproject.toml`, and the tests written in python. A rust test
      binary for an extension module has no interpreter to run in. **The state machine, not
      the core**, which settles the open question above: the runner drives `CrowFlies` and
      knows nothing of crux. An instant crosses as an aware `datetime` through pyo3's `chrono`
      feature, so a naive one is a `TypeError` rather than some other moment.
- [x] Read a named session's samples from silver in python, ~~with DuckDB over the store as
      `visualise` does today~~, and feed them through the extension in `t` order. **Through
      `medallion-py` instead**, which grew a `query_silver`: DuckDB in python would have made
      `runner/store.py` a third place that knows the store's layout, after `medallion` and
      `visualise`. Reading a dataset by name brings its partitions and its geometry's CRS with
      it, so a crossing's position arrives as coordinates rather than as WKB to decode. Behind
      it, `medallion::Query` grew bound parameters, and a result that always carries its
      columns — an empty batch where a query matched nothing.
- [x] Derive the datasets a query reads from the query itself, so `query_silver` takes SQL
      alone and `datasets=` goes away. Parse rather than match on the text ourselves:
      datafusion already carries a parser, and `resolve_table_references` in `datafusion_sql`
      answers a parsed statement's table references, separating them from its CTE names. A
      reference naming no silver dataset is then the error `datasets=` catches today. It
      lands as `medallion::table_references`, which leaves naming the datasets to `model`
      where it already lives.

- [x] Log the track, each prediction as it is made, and its error against the crossing when
      that crossing arrives. **The error is a series rather than one number**: logged at every
      fix that predicts a crossing whose passing is known, so a prediction converging on the
      water and one that never does look nothing alike over a run. Arrival is
      `session_crossing`'s, read by `Store.passings`, so the runner does not invent a second
      definition of having passed. Drawing goes through the recording it is handed rather than
      the one rerun holds globally, so standing in for a recording reads back every entity
      path and value drawn — which is the only read-back there is, rerun offering none of its
      own. `just sessions` and `just replay <id>` are the commands.
- [x] Log silver `session_crossing` as the ground truth to compare against. Three ways: the
      crossings a session reached are drawn apart from the rest on the map, each passing is
      noted at the moment it happened with how near the sample that matched it was, and the
      true countdown to a passing is drawn against the predicted one, so a plot holds both
      and the gap between them is the error.
- [x] Give it a blueprint: a map of the session and the crossings, and a timeline of
      predicted times against actual ones. **No view names a crossing**, so a session with two
      crossings and one with two thousand lay out the same way — which is why the entity paths
      group by what is measured rather than by which crossing it is measured against: a plot is
      one subtree, and a crossing is a series within it.
- [ ] Change to a very simple rerun usage, that is easier to understand even if it doesn't look fancy:
      * [x] show samples and predictions
      * [x] get rid of any cruft we don't need now which includes code no longer used in log.py/draw method
      * [x] reduces tests down to just being very simple i.e. just verifying that when something is replayed and predictions made, we log something to these streams:
            * "steps/log"
            * "steps/sample/position"
            * "steps/sample/positions"
            * "steps/predictions"

      The stream names are constants in `log.py`, so the blueprint and the tests name the same
      thing the drawing does. **Everything the deferred plot would have needed is deleted
      rather than kept**: `Store.passings` and the `Passing` it answered, the
      `session_crossing` rows in the fixture, and — dead once nothing drew a trend —
      `Trending`, `Trend` and their tests, in the python binding and in `predictor` alike.
      Git is the archive.
- [-] **Deferred**, to be picked up on its own rather than here: the simple rerun usage above
      is what this slice ends with, and the ground truth this needs has been deleted along
      with everything else the plot alone would have used. Draw the predictions as a 2D plot
      of predicted against actual: x is when a crossing was
      really passed and y is when the latest fix expects to pass it, both measured from the
      start of the session. A crossing sits at a fixed x and moves up or down as
      the prediction changes, so a perfect predictor puts every crossing on the y = x
      diagonal, over-prediction above it and under-prediction below. Mark each real passing on
      the x axis, so the truths read even where nothing is predicted against them. This is the
      view of the thing the slice is about; the map answers *where*, and this answers *how
      wrong*.

      **A prediction past the end of the session grows the y axis** rather than being clamped
      to it or dropped: at a standstill the arrival runs far into the future, and a point
      sitting high above the diagonal is the honest picture of that. So the axes are equal
      scales rather than a fixed 0 to 1 — seconds since the session started will do, as long
      as both axes measure the same thing, which is what keeps the diagonal meaningful.

      **A crossing with only one half is left out.** One predicted but never passed has no x,
      and one passed but never predicted has no y; neither belongs on a time-against-time
      plot. Both are worth seeing — a prediction with no crossing behind it is exactly the
      failure this is for — but on a plot of their own later, not by bending this one.

      Unlike a map view, a `Spatial2DView` takes a visible time range, so the trail a crossing
      leaves as it converges can be shown as well as its latest position.

### 6. Delete what is replaced

- [x] Delete `visualise/` and its `just visualise` recipe. The bronze GPS and accelerometer
      views and the moving `train_segment` dots go with it. The rerun runner draws the
      predictor and nothing else. `docs/architecture.md` named it twice and now names the
      runner. **`just test-python` runs in the sandbox again**: the suite that could not
      complete there was `visualise`'s, which needed a DuckDB extension the sandbox refuses to
      install.
- [x] Delete `spikes/m5/`, after checking [device.md](device.md) carries every board fact
      worth keeping. One was missing and is now recorded: the Grove port is G32/G33 and never
      UART0, which is the USB console. The `just random-crossings` recipe went with them,
      since it wrote its point set into spike 5; `just carried-crossings` is the one that
      feeds the device.
