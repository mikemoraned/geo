# Next Slices

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


## Slice: upgrades

### Target

Pinned versions are how this repo stays reproducible, and the cost is that they age quietly
until something forces a move. The move is then taken mid-slice, under pressure, with no way to
tell an upgrade's breakage from the day's work: marimo went 0.23.15 to 0.25.0 that way, in the
middle of the UK slice, because a sandbox stopped resolving. This slice is the scheduled version
of that — find what is behind, move it deliberately, and record what each move cost.

One upgrade is already owed, and it is holding work back.

**SedonaDB 0.4.0 panics scanning a wide bronze partition.** `index out of bounds: the len is 7
but the index is 18`, in `rust/sedona-expr/src/spatial_filter.rs:558`, where geometry statistics
are indexed by a column's position in the *file* schema against statistics gathered for the
*projected* one. It bites when the geometry column sits past the projected column count:
`transportation/segment` carries geometry at index 18 and dies, `divisions/division_area` carries
it at index 1 and reads. Apache SedonaDB issue
[#389](https://github.com/apache/sedona-db/issues/389), "Parquet pruning expressions should be
evaluated against the projected schema and not the file schema", is the same mistake and is
closed by PR #385, and 0.4.1 is released. Until that lands here, bronze geometry is read as plain
parquet and decoded from WKB, which is what `Query::register_at_without_geometry` is for.

### Tasks

- [ ] Report what is behind, in one command: the rust dependencies, the python ones each notebook
      pins, and the toolchain. What the report costs to run decides how often an upgrade is
      considered at all.
- [ ] Take SedonaDB to 0.4.1 or later, and scan `transportation/segment` through the geo reader
      to see whether the panic is gone. Where the plain-parquet read was only a way round it,
      drop it; where it is the honest read — a partition value, a column with no CRS to declare —
      keep it.
- [ ] Move arrow, parquet and datafusion with SedonaDB. The workspace comment pins them to the
      generation SedonaDB builds against, so they are one decision rather than four.
- [ ] Pin the ESP toolchain, which `rust-toolchain.toml` leaves as `channel = "esp"`. A device
      build therefore moves under us, and
      [2026-09-21-m5-reboots.md](2026-09-21-m5-reboots.md) already recommends pinning it whatever
      the cause of that fault turns out to be.
- [ ] Say in each upgrade's commit what it cost: what broke, what was rewritten, and what a
      reader would otherwise mistake for the feature it travelled with.

