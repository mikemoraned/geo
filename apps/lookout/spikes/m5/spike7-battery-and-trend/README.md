# Spike 6 — the real water crossings

Spike 5 with the made-up points replaced by the real ones: **5,749 places a railway meets
water in Germany**, derived from Overture by the water-crossings notebook. Spike 5 answered
what a scan of that many points costs; this one answers whether the answers are *right*, and
whether they behave sensibly while moving.

```sh
just test           # run the core's tests on the laptop
just flash-release  # build, flash, and tail the serial console
```

The code is spike 5's, unchanged. What differs is the file in `core/src/`.

## The device agrees with the notebook

The check this spike exists for. The five crossings nearest Dresden Hauptbahnhof, and their
distances, were worked out from the source GeoParquet with an independent `f64` haversine —
then asserted against what the core returns from the packed `f32` buffer:

| id | notebook (`f64`) | device (`f32`) | difference |
|---|---|---|---|
| `2620a981` | 2334.9 m | 2335.17 m | 0.27 m |
| `6ad4b654` | 2338.5 m | 2338.58 m | 0.08 m |
| `0ea20750` | 2343.1 m | 2343.27 m | 0.17 m |
| `e6c6312b` | 2347.3 m | 2347.53 m | 0.23 m |
| `4efedc58` | 2351.6 m | 2351.79 m | 0.19 m |

Same crossings, same order, **agreeing to 0.27 m over 2.3 km** — about 1 part in 10,000. Both
sides also count **20 crossings within 5 km**, which is the stricter check of the two: a
distance can be a little out and still be right about what is nearest, but a membership
question can flip.

That covers the whole chain at once: GeoParquet → WKB decode → `f32` narrowing → packed
columns → cast in place → `geo`'s haversine. The residual is about what `f32` coordinates cost
(~0.2 m at this latitude) plus the two implementations using slightly different mean earth
radii.

They are, as it happens, five crossings of the same braid of the Elbe a couple of kilometres
north of the station — which is what the dataset should say about Dresden.

## What the panel shows

```
20:43:29        clock, from the fix's own UTC
51.05821        latitude
13.73410        longitude
8sat h2.4       satellites and HDOP
3 in 5km        crossings within 5km — the predictor's half of the scan

2620a9 2.3km    the five nearest, nearest first
6ad4b6 2.3km
0ea207 2.3km
e6c631 2.3km
4efedc 2.4km
```

Fix quality is on the panel, not just in the log, because in the field the panel is the only
output — the serial console needs a laptop. Without it there is no way to tell a distance that
is jittering from a *fix* that is jittering, and spike 3 measured how much that matters: 8
satellites at HDOP 2.4 wanders about a metre, but 6 at HDOP 4.4 wanders 4.5 m/s and reports a
false 4-knot speed with it.

## The lat/lon representation held up

`f32` degrees were chosen on a measured ≤0.21 m round-trip error over this set. Against real
distances that lands as **0.27 m of disagreement over 2.3 km**, roughly 1 part in 10,000 — far
under the metre-scale wander a stationary fix shows even in good conditions, and nowhere near
enough to reorder two crossings that are metres apart. `i32` at 1e-7° would have bought
centimetres nobody can use, for a third more flash.

## What a scan costs: ~4.7 ms for 5,749 crossings

Measured in spike 5, on made-up points of the same size, and unchanged here — the cost is set
by how many points there are, not what they mean.

| profile | `opt-level` | `debug-assertions` | per scan | per crossing |
|---|---|---|---|---|
| dev (`just flash`) | `z` | on | 7,835–8,377 µs | ~1.39 µs |
| release (`just flash-release`) | `s` | off | 4,353–5,025 µs | ~0.82 µs |

**Release is 1.7× faster than dev**, and most of that is unlikely to be the opt-level — `z`
and `s` are both size-oriented — but the `debug-assertions` and `overflow-checks` the dev
profile turns on, which put a branch on every arithmetic operation inside the haversine.
Measure on release; the dev number is what iteration feels like, not what the device does.

At 0.82 µs a crossing — about 196 cycles at 240 MHz for an f32 haversine plus the top-N
bookkeeping — a scan is **0.5% of a one-second budget**. Both figures include parsing the
sentence that carried the new position, so they are upper bounds on the scan alone.

Brute force is settled at this size and stays settled well past it: ~120,000 points before a
scan reaches a tenth of the fix interval, ~1.2M before it fills it. The German set would have
to grow **20× before an index is worth discussing**.

### The prediction held, but its reasoning did not

The slice predicted single-digit milliseconds, and 4.7 ms is single-digit milliseconds. But it
got there via "even 50k points (~400 KB) scans in single-digit ms at 240 MHz", and at the
measured rate 50k would take **~41 ms** — still an order of magnitude out. The conclusion
survived only because the real set turned out to be 9× smaller than the number the estimate
was built on. Worth remembering before reusing that reasoning for a bigger set: on this board,
budget most of a microsecond per point, not tens of nanoseconds.

## Footprint

| | |
|---|---|
| packed set | 69,000 bytes (`12 + 12n`), in flash via `include_bytes!` |
| scan working set | the top five and whatever is within 5 km — the columns are never copied |

Built into the binary rather than read from a filesystem: at 69 KB against 8 MB of flash the
saving would be nothing, and a filesystem would cost a partition table, a mount at boot, and a
way for the device to end up holding a set that disagrees with the code reading it.

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
NimBLE running — but because the predictor this core is a step towards does. Building against
0.19 now would mean discovering on the way back that the core has to come down again. The pin
costs one associated type (`type Capabilities = ()`, dropped in later versions) and a `_caps`
argument to `update`. Spike 4's README has the evidence.

## The console

Spike 3 echoed every NMEA sentence, so a monitor session doubled as a capture. This one does
not: a dozen-plus sentences a second bury the one line worth reading, which is how long a scan
took. Go back to `spike3-gnss` and its `just capture` if more fixtures are needed.
