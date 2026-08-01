# Next Slices

## Slice: Crow-flies predictor deployed to M5 device and Rerun sim

### Target

One half of this is a clean-up / rationalisation of what we've already spiked on with the M5 device (apps/lookout/spikes/m5/spike7-battery-and-trend). The other half is turning rerun visualisation into something that shows what the predictor is doing.

Effectively what I want to end up with is:
* A core predictor defined in a CRUX wrapper, perhaps with it's central core being a state-machine
* A productionised version of the M5-deployed setup that uses that core to read live GPS readings and predict when water will be crossed
* A rerun-based simulation that re-drives a named session (from silver/session table) as fake GPS readings through the predictor, captures the predictions, and visualises them
  * I want to use this as way to see how the predictor is performing by live-comparing where it thinks it's going to cross water vs what water is actually there

As part of this we should also be able to delete all the current `visualise` code + remove the `spikes/m5` dir.

## Slice: Evaluation framework based on sampled sessions from myself and motis

### Target

Implement an evaluation framework which uses advice from apps/lookout/docs/2026-08-01-evaluation.md and applies it to saved sessions from myself (silver/session table) and from motis (bronze/motis_segment). The idea is to use real recorded data from being on a train or from reported positions of trains to drive an evaluation of what the predictor says about future water crossings compared to when they actually happened. We can use silver/session_crossing for this, and we may want to apply the same pattern to motis data i.e. treat motis train tracking as a session.

Since I likely won't be in Germany for a while, if needed, we can get new motis data by live polling motis in a particular bounding box, and just watching when trains arrive.

#### Tasks 

...
- [ ] Delete `docs/2026-08-01-evaluation.md` at the end of this slice. It is a dated
      assessment of how to measure a predictor, written before one existed and before there
      was any ground truth to measure against, and kept for the history of the decision.
      Anything in it still holding by then belongs in the tasks above, in the notebooks
      that implement the measures, or in a durable doc alongside `medallion.md`; the rest —
      the rejected alternatives, the reasoning about metrics from other domains — goes
      stale once a first run has actually produced numbers.


## Slice: make the store operable at size

### Target

The store's layout is settled; what it lacks is the ability to be *worked* — to re-derive
part of history rather than all of it, and to stop accumulating files without bound. Both
become urgent at a size we are not at yet, and both are cheaper to build before then.

### Refactors / extensions

- **Give the derivation CLIs a date-range argument**, so a run can ask for less than
  everything. They currently read every partition and filter on data columns, which means
  the partition pruning the layout provides is never exercised: re-deriving one day's output
  costs a full scan. This is also the prerequisite for handing the work to an orchestrator
  later, since a range is what a backfill is expressed in.
- **Write down a compaction plan for the append-shaped layers**, before the small-file
  problem is real rather than after. One file per ingestion is deliberate and correct at the
  point of writing, but a dataset polled on an interval accumulates a file per poll
  indefinitely (the sqlite backfill alone produced 1,307 in one dataset). The standard answer
  is periodic compaction into fewer, larger files per partition; the thing to decide is what
  triggers it and how it preserves immutability, since rewriting files is what that layer
  forbids.
- **Leave the engine catalog traits alone** until registering datasets by hand is genuinely
  annoying, then add a schema provider *over* the dataset definitions rather than replacing
  them. The definitions are plain data every engine can read; a catalog is one engine's view
  of it, and those traits move between that engine's releases.

## Slice: embed predictor on website

### Target

We now want to take our simple predictor and start applying it for real.

### Straw Man

This should involve refactoring the existing lookout fly.io website so that it's sensor gathering follows the crux / ports-and-adaptors pattern. Then we can extend it to apply the predictor and visualise it in a simple way.

This also is where we need to be publishing data about crossings for it to download client-side e.g. a PMTiles file.

### Tasks

...

## Slice: extend to UK

### Target

We've mostly been testing with German (DE) data. We should repeat / extend what we did in Germany on the UK train network.

### Refactors / extensions

- **Silver datasets holding geometry need partitioning by country.** The projected geometry
  column carries one CRS, chosen per country (`medallion::Country`), but a dataset like
  `train_segment` is partitioned only by date — so segments from two countries would share
  a partition and the dataset would end up with a different CRS per file depending on which
  run wrote it. Partitioning by country as well (`country=<iso>/departure_date=<date>`)
  keeps each partition to one CRS. That needs `DatasetSpec` to carry more than one
  partition key, which it currently does not.
- Add the UK to `medallion::Country`: its projected zone (British National Grid, EPSG:27700)
  and the PROJJSON for it, generated by `just crs-definitions`.

...

## Slice: Deploy predictor on M5 device

### Target

Run the real predictor on the M5StickC PLUS2, fed by its own GPS unit rather than by replayed
traces — the point the device spikes were building towards.

### Constraint inherited from the device spikes: `crux_core` is pinned

**`crux_core` must stay pinned at `=0.16.2` on device.** On 0.19, Crux + BLE reboots the
M5 every 4 seconds to 7 minutes, always inside crux's per-effect `Command`/crossbeam machinery.
Pinning 0.16.2 fixes it — 30 minutes stable with a client connected and a real fix — but the
cause was never identified, only avoided; it is some change between 0.17 and 0.19. Stack
overflow, both task stacks, heap exhaustion, fragmentation, heap overrun, PSRAM, allocation
volume and model placement were all ruled out by measurement. Evidence and the four wrong
diagnoses are in `apps/lookout/spikes/m5/spike4-ble/README.md`.

Two consequences for this slice:

- The predictor core has to compile against 0.16.2's API (one extra associated type,
  `type Capabilities = ()`), so don't adopt newer crux features in the shared core.
- If a newer crux becomes necessary, the work is bisecting 0.17/0.18 to find the change (one
  flash and a 15-minute soak each), or dropping crux from the device shell — which would
  forfeit the shared-core argument that justified Crux in the first place.

### What the spikes already established

The spikes leave a working skeleton to hang the predictor on: `apps/lookout/spikes/m5`, with
a Crux core split from an esp-idf shell so the core stays testable on the laptop. The
predictor should *be* that core. See `.claude/memory/m5-esp32-toolchain.md` for the board's
gotchas (power hold, panel offset, RX pin, stack size, UART buffer).

### Notes & Gotchas (what the on-device GNSS actually behaves like)

Measured on the GPS/BDS unit (AT6668) with the receiver deliberately held still:

- **Good geometry** (8 satellites, HDOP 2.4): position wanders ~1.8m horizontally over 12s,
  and differencing consecutive one-second fixes implies ~1.0 m/s of motion. The receiver's
  own Doppler-derived speed is far more honest at 0.04–0.91 knots (≤0.5 m/s).
- **Poor geometry** (6 satellites, HDOP 4.4): position wanders ~4.5 m/s, *and* the reported
  speed claims 4.13 knots. In poor conditions the Doppler speed lies too.

So a predictor that derives velocity by differencing distance-to-crossing between successive
readings — as the minimal predictor's straw man does — will be dominated by noise at these
scales, and a stationary device can appear to be closing on a crossing fast enough to emit
confident nonsense. Two responses, both wanted here:

1. Prefer the receiver's reported speed over position-differencing, and/or difference over a
   longer baseline than one reading.
2. **Gate on fix quality.** HDOP and satellite count are the difference between ~0.2 m/s and
   ~4.5 m/s of phantom motion, so the on-device predictor needs them as inputs, not just
   lat/lon. `RMC`/`GGA` carry both.

A train at speed should swamp this noise, but approach and departure — exactly when a crossing
prediction is being refined — are the low-speed regime where it bites. Worth checking whether
the same holds for the phone traces before assuming it is device-specific.

Also: the receiver reports true UTC in `ZDA`/`RMC` before it has any position fix, so
wall-clock time needs neither NTP nor the BM8563 RTC.

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. This allows a recommendation to be given about which direction to look relative to the train seat.

## Slice: Adding POI's from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POI's by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to to identify exactly what the POI is based on the image.