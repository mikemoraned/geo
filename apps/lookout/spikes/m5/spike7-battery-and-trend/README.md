# Spike 7 — battery (and, later, which way a crossing is going)

Spike 6 with a battery indicator. Both this and a closing/holding/receding marker came out of
spike 6's field check: untethered, the panel is the whole interface, and a list of distances
does not say whether a crossing is about to be passed or has just been missed.

**The trend half is deferred, not dropped** — judging it needs a moving train, and there is no
travel for a few weeks. The directory keeps its name for when it resumes; the tasks are in
`apps/lookout/docs/current-slice.md`, marked `[-]`.

Everything spike 6 does, it still does: the real 5,749 crossings, scanned against each fix.

```sh
just test           # run the core's tests on the laptop
just flash-release  # build, flash, and tail the serial console
```

The code is spike 5's, unchanged. What differs is the file in `core/src/`.

## The battery

Drawn as four coarse steps at the right-hand end of the clock's line — `[===]` down to
`[   ]` — in the five characters the clock leaves of thirteen. A percentage would imply
precision the measurement has not got: a lithium-polymer cell sits near 3.7 V for most of its
life, so the middle of the curve is nearly flat.

**Read in the shell, judged in the core**, the same way NMEA is: the shell reads the pin and
sends `Event::Battery(millivolts)`; what a voltage *means* lives in `core/src/battery.rs`. The
payoff is that a whole discharge — 4.2 V down to 3.2 V — runs as a unit test, checking that the
indicator never reads fuller as the voltage falls, that every step is reachable, and that a
reading sitting on a boundary does not flicker. On hardware that experiment takes an hour and a
half and cannot be repeated on demand.

### Where the numbers came from

From M5's own board table (`M5Unified/src/utility/Power_Class.cpp`, `board_M5StickCPlus2`)
rather than guessed: **GPIO38 on ADC1, 12 dB attenuation, 12-bit, divider ratio 2.0**, and
`pmic_adc` — there is no PMIC to ask, which is the other face of the PLUS2 having no AXP192.
The same table gives GPIO4 as this board's hold pin, which is what spike 0 established on
hardware: a cheap check that the right board is being read. Calibration is **line fitting**;
curve fitting is a C3/C6/S3 feature, and line is the fallback M5's own code takes here.

The raw millivolts go to the console once a minute, because the bars are deliberately too
coarse to check a divider or a calibration against.

### Two things about `battery-estimator`

The curve comes from the crate, not from a table fitted here. Two things worth knowing:

- its `default_curves::LIPO` is **unreachable** — the module is private. `BatteryChemistry::LiPo`
  is the public way to the same curve.
- it **clamps out-of-range voltages rather than refusing**; there is no range error in its API
  at all. A disconnected pin reading near zero would come back as a confident 0%, and a misread
  of 9 V as a confident 100%. A plausibility range in front of it turns both into saying
  nothing, which is what a blank indicator means here.

`m5unified` was the other candidate and knows this board properly, including a `battery_level()`
for the PLUS2. It was rejected because `M5Unified::begin()` initialises the display too — there
is no power-only path — so one number would cost the panel driver spike 1 tuned, plus a C++
ESP-IDF component. Worth revisiting if the IMU, RTC, and buttons are ever wanted as well.

## Everything else is spike 6's

The crossings check against the notebook, what the panel shows, the scan cost, and the
footprint are all spike 6's and unchanged here — see
[`spike6-crossings/README.md`](../spike6-crossings/README.md). The one difference on the
panel is the battery, at the right-hand end of the clock's line:

```
20:43:29 [===]  clock from the fix's own UTC, and the battery
```

## The point set is regenerable, but not from this branch

```sh
cargo run -p crossings --bin pack_crossings -- --input <crossing_reps.parquet> \
    --output spikes/m5/spike7-battery-and-trend/core/src/water-crossings.pointset
```

The `crossing_reps.parquet` it reads is the water-crossings notebook's own output, which is
gitignored as regenerable and lives wherever that notebook was last run. The committed
`.pointset` is therefore the artifact of record here. See `crates/crossings/README.md` for the
byte layout and what a reader must check.

## `crux_core` is pinned to `=0.16.2`

Not because this spike needs it — there is no BLE here, and the reboot only appears with
NimBLE running — but because the predictor this core is a step towards does. See
[`docs/device.md`](../../../docs/device.md); spike 4's README has the evidence.

## The console

Spike 3 echoed every NMEA sentence, so a monitor session doubled as a capture. This one does
not: a dozen-plus sentences a second bury the one line worth reading, which is how long a scan
took. Go back to `spike3-gnss` and its `just capture` if more fixtures are needed.
