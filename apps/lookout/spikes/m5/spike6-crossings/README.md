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

## What a scan costs

Unchanged from spike 5, which measured it: the cost is set by how many points there are, not
by what they mean. See [`docs/device.md`](../../../docs/device.md).

## Footprint

Measured on the release ELF with `xtensa-esp32-elf-size`:

| | |
|---|---|
| packed set | 69,000 bytes (`12 + 12n`) |
| firmware, flash | 764,694 bytes (539,139 text + 225,555 data) — **under 10% of the 8 MB** |
| `.flash.rodata`, which carries the set | 212,256 bytes |
| static RAM (`.bss`) | 7,882 bytes |
| heap per scan | a few hundred bytes: `Near` is 16 bytes, so the five nearest cost 80 and the twenty within 5 km of Dresden 320 |

The set is **verifiably in the image**: searching the release binary for the format's magic
finds it with a valid header — version 1, count 5,749 — at file offset `0x109f4`.

Nothing copies the columns out of flash. `PointSet` borrows them where they lie, so a scan
allocates only for what it reports, and holding a bigger set would cost flash rather than RAM.
That is the property that makes the size question a flash question.

Built into the binary rather than read from a filesystem: at 69 KB against 8 MB the saving
would be nothing, and a filesystem would cost a partition table, a mount at boot, and a way for
the device to end up holding a set that disagrees with the code reading it.

The shell also logs free heap and stack high-water beside each scan, so a tethered run can
confirm the figures above hold while running. That log has not been captured yet — the field
run was deliberately untethered — and the numbers here are static ones, from the binary and
from the code.

## The point set is regenerable

```sh
just gold-pack-crossings --output spikes/m5/spike6-crossings/core/src/water-crossings.pointset
```

It reads the silver `water_crossing` dataset out of the medallion store, so regenerating it
needs that dataset to have been derived. The committed `.pointset` is what this spike was
measured against either way. See `crates/crossings/README.md` for the byte layout and what a
reader must check.

## `crux_core` is pinned to `=0.16.2`

Not because this spike needs it — there is no BLE here, and the reboot only appears with
NimBLE running — but because the predictor this core is a step towards does. See
[`docs/device.md`](../../../docs/device.md); spike 4's README has the evidence.

## The console

Spike 3 echoed every NMEA sentence, so a monitor session doubled as a capture. This one does
not: a dozen-plus sentences a second bury the one line worth reading, which is how long a scan
took. Go back to `spike3-gnss` and its `just capture` if more fixtures are needed.
