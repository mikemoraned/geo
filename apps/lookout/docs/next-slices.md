# Next Slices

## Slice: Crow-flies predictor deployed to M5 device and Rerun sim

### Target

One half of this is a clean-up / rationalisation of what we've already spiked on with the M5 device (apps/lookout/spikes/m5/spike7-battery-and-trend). The other half is turning rerun visualisation into something that shows what the predictor is doing.

Effectively what I want to end up with is:
* A core predictor defined in a CRUX wrapper, perhaps with its central core being a state-machine
* A productionised version of the M5-deployed setup that uses that core to read live GPS readings and predict when water will be crossed
* A rerun-based simulation that re-drives a named session (from silver/session table) as fake GPS readings through the predictor, captures the predictions, and visualises them
  * I want to use this as a way to see how the predictor is performing by live-comparing where it thinks it's going to cross water vs what water is actually there

As part of this we should also be able to delete all the current `visualise` code + remove the `spikes/m5` dir.

## Slice: Evaluation framework based on sampled sessions from myself and motis

### Target

Implement an evaluation framework which uses advice from apps/lookout/docs/2026-08-01-evaluation.md and applies it to saved sessions from myself (silver/session table) and from motis (bronze/motis_segment). The idea is to use real recorded data from being on a train or from reported positions of trains to drive an evaluation of what the predictor says about future water crossings compared to when they actually happened. We can use silver/session_crossing for this, and we may want to apply the same pattern to motis data i.e. treat motis train tracking as a session.

Since I likely won't be in Germany for a while, we can if needed get new motis data by polling motis live in a particular bounding box and watching when trains arrive.

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

This should involve refactoring the existing lookout fly.io website so that its sensor gathering follows the crux / ports-and-adaptors pattern. Then we can extend it to apply the predictor and visualise it in a simple way.

This also is where we need to be publishing data about crossings for it to download client-side e.g. a PMTiles file.

### Tasks

...

## Slice: extend to UK

### Target

We've mostly been testing with German (DE) data. We should repeat / extend what we did in Germany on the UK train network.

### Refactors / extensions

- Add the UK to `medallion::Country`: its ISO code, its projected zone (British National
  Grid, EPSG:27700), and the PROJJSON for it, generated by `just crs-definitions`. A test
  already holds every country's bundled PROJJSON to the EPSG code it names, and `ALL` is
  what a sweep of the country partitions iterates, so both follow from the new variant.

Partitioning geo silver by country needs no work: the country level is applied above a
dataset's own partition key by the shared silver write path, not declared per dataset, so a
second country lands in its own partitions and its own CRS as soon as `Country` knows it.

...

## Slice: Deploy predictor on M5 device

### Target

Run the real predictor on the M5StickC PLUS2, fed by its own GPS unit rather than by replayed
traces — the point the device spikes were building towards.

### What the spikes already established

The spikes leave a working skeleton to hang the predictor on: a Crux core split from an
esp-idf shell, so the core stays testable on the laptop. The predictor should *be* that core.
Everything the spikes established about the board — power hold, panel offset, RX pin, stack
sizing, UART buffer, and what the receiver's numbers are worth — is in [device.md](device.md),
so the spike code itself can go.

Three things there bear directly on this slice:

- **Build the predictor core against the current `crux_core`**, and soak it on device with
  BLE running. 0.19 rebooted the board every few minutes and 0.16.2 did not; whether later
  releases still do is unknown. [device.md](device.md) has the evidence and what dropping
  back would cost.
- **The predictor should take fix quality as an input, not just lat/lon.** Held still in
  poor geometry the receiver showed metres per second of phantom motion and a false
  multi-knot speed, so a straw man deriving velocity by differencing position between fixes
  may emit confident nonsense from a stationary device. That rests on two observations only
  — see [device.md](device.md) — so it is a reason to design against the failure, not an
  established characterisation of the receiver.
- **Wall-clock time can come from the receiver**, before any position fix, so the device
  needs neither NTP nor the BM8563 RTC.

Worth checking whether the phone traces show the same noise before assuming it is specific
to this receiver.

## Slice: rail track geometry from pfaedle (parked)

### Target

Give rail legs real curved geometry instead of the straight stop-to-stop lines DELFI's
`shapes.txt` yields for rail — see [motis.md](motis.md). pfaedle map-matches GTFS trips onto
OSM to synthesise `shapes.txt`, and produced correct curved rail: `-D -m rail` recomputes
rail shapes only, leaving bus and coach shapes alone, and rail polylines come out hundreds
of points where they were four.

**Parked, and the tooling was reverted out of the tree** (`tools/pfaedle`, commit
`dfd8655`), because importing the result breaks realtime.

### Why it is parked

Import the raw DELFI feed and around 99.97% of RT entities resolve. Import any feed carrying
pfaedle's `shapes.txt` and trip resolution fails for ~99.6% of them, with **no segment coming
back realtime-corrected**. The static schedule itself imports fine: the trips are there and
the rail is genuinely curved.

It is the `shapes.txt` and not the trips. Three attempts broke realtime identically,
including one that kept `trips.txt` byte-identical to the raw feed apart from the rail
`shape_id` fields — and there the failing trips were *bus* trips whose `shape_id` was never
touched. The only remaining difference is the swapped-in `shapes.txt`, which grows from
308 MB to 2.3 GB. It is not feed currency either: a same-day RT fetch still overlapped the
static feed's trip ids 99.6%.

Leading hypothesis, untested: the 2.3 GB of rail geometry makes `motis import` hit some
limit and produce a timetable whose RT trip index is incomplete, while scheduled queries
still work.

### To resume

1. Confirm the trigger — build the raw feed with only `shapes.txt` swapped, import, and check
   the RT statistic. Expect it to break.
2. Chase the cause: read `motis import` for shape, memory or limit warnings; try shrinking
   `shapes.txt`, by simplifying the rail polylines or dropping the unused bus shapes, and
   re-test.
3. If Motis genuinely cannot take large rail shapes, file an issue upstream, or accept
   straight-line rail — which is what transitous does — and drop this.

Two facts about pfaedle worth keeping if it resumes: it has no homebrew formula and has to
be built from source against `cmake` and `libzip`, and it must run from its build directory
with an explicit config path, since it only finds its default MOT-to-OSM matching config
when installed. Its GTFS parser is also stricter than Motis's — it aborts on the dangling
references in DELFI's `transfers.txt` and `pathways.txt`, which Motis tolerates.

Each realtime A/B needs the Motis server run by hand: the sandbox denies the LMDB tile mmap.

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. That allows a recommendation about which direction to look from the train seat.

## Slice: Adding POIs from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POIs by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to identify exactly what the POI is based on the image.