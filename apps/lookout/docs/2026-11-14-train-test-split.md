# Splitting sessions for tuning and testing

*Investigation of 2026-11-14, written before LightCone reads its first setting from data. It sets
out the options and the experiments that choose between them. What holds after those experiments
belongs in `docs/evaluation.md`. The rest goes stale.*

This doc covers how the evaluation keeps the sessions a predictor is tuned on apart from the
sessions it is tested on. It is for whoever chooses a predictor's settings from runs in gold.

**Tuning** is anything that sets a predictor's settings from sessions. It takes two forms here:

- **Measuring a setting from the data.** LightCone bounds its cone's angle by the angles trains
  turn through within N, read from recorded sessions.
- **Choosing settings from a sweep.** Several runs replay the sessions under different settings,
  and the best score picks one. CrowFlies' radius was chosen this way.

A predictor tested on the sessions it was tuned on scores better than it will on sessions it has
not seen. The evaluation needs a test set that no tuning has read.

## Where the evaluation leaks now

`pick` orders every session that passes the filter by the SHA-256 of the seed and the session's id.
It keeps the first N. Nothing sets sessions aside, so runs under two seeds overlap, and a run over
every session contains every other run.

CrowFlies' 1 km radius won a sweep over the sessions every later run replays, as
[the radius report](evaluations/2026-11-10-crow-flies-radius.md) records. The cooldown run compared
against that same pool. The best of several runs scores high on those runs partly by chance. Only
sessions outside the sweep show how much.
[Cawley and Talbot](https://jmlr.org/papers/v11/cawley10a.html) show this selection bias to be as
large as the differences between the methods they compare.

## Why a random split of sessions is not enough

A session is one trip on one day. Sessions repeat each other in three ways:

- **One trip on two days.** The sessions the filter keeps cluster on 2026-10-18 and 2026-10-19,
  under one timetable. Most trips run on both days, over the same track at the same times.
- **One line through the day.** A line with a train every 20 minutes gives dozens of sessions a
  day over the same track. The slice's decisions already name this as the reason its intervals come
  out too narrow.
- **One corridor, several lines.** An ICE and a regional train can share track between two cities.

What LightCone learns is how far trains turn within N, and that is a property of the track. Say a
split puts the 10:00 train in the tune set and the 10:20 train in the test set. It then tunes and
tests on the same curves. [Roberts et al.](https://doi.org/10.1111/ecog.02881) show that a random
split of data with this kind of structure overstates how well a model does on new data. Their
remedy is to split by group, or by block of space or time, so that each group falls wholly on one
side. [Kaufman et al.](https://doi.org/10.1145/2382577.2382579) class a split that ignores such
groups as leakage.

## Two sets: tune and test

- **Tune**: the sessions settings are measured from, and the sessions every sweep runs over.
- **Test**: the sessions each predictor's chosen settings run over once, to compare predictors.

Machine learning commonly splits three ways. A training set fits a model's weights, a validation set
chooses its hyperparameters, and a test set checks the result. This slice needs no third set.
LightCone's measured angle only bounds the sweep of angles, and the sweep makes the choice. Both
forms of tuning therefore choose among a few settings, and both run over the tune set. A predictor
that learns many numbers from the data, as a model's weights are learned, brings the third set back.

A test set that informs a choice becomes a tune set. Each comparison on it is then a sweep, with the
selection bias above. Phase 7 reads the test set to choose the apps' predictor, so a later predictor
compared on that test set inherits that bias. A fresh test set, such as a later capture, removes it.

Splitting the sessions widens every interval. An interval's width falls with the square root of the
number of sessions. A test set of 30% of the 34,521 sessions in the widest runs so far holds about
10,400 sessions. Its intervals come out about 1.8 times wider than those runs'.

## Assigning sessions to a set

[*Machine Learning Design Patterns*](https://www.oreilly.com/library/view/machine-learning-design/9781098115777/)
describes **repeatable splitting**: hash a grouping column, and assign each group a set by the hash
modulo 100. A group whose hash falls below 70 goes to tune, and the rest to test. The assignment
depends on the hash alone, so it holds across reruns and library upgrades.

`pick` already hashes in the engine, so it can assign sets the same way. A `Pick` gains the set it
draws from. The engine hashes a salt with each session's group key, keeps the sessions in that set,
and then orders them by the seed as now. The salt is fixed, so every run sees the same sets.
`evaluation_run_v5` records the set and the group key. The run's id hashes them with the rest of its
settings. Runs before the split migrate with no set recorded.

[scikit-learn's grouped splitters](https://scikit-learn.org/stable/modules/cross_validation.html#cross-validation-iterators-for-grouped-data),
such as `GroupShuffleSplit`, do the same in a notebook. They draw from a seeded generator, so a
version upgrade can move a session to another set. That rules them out for the store's runs, though
they serve to check `pick`'s sets.

## Group keys

The key follows what a predictor claims to generalise to. LightCone claims to work on any track, so
its test set holds track its tune set lacks. Each key below keeps more track on one side of the
split than the key before it.

### Trip

A trip keeps one trip's two days together. It leaves the 10:00 and 10:20 trains of one line in
different sets, so it removes the least leakage. Motis prefixes each `trip_id` with its date, as in
`20261019_14:05_germanygtfs_3440430398`. A key that holds a trip across days therefore drops that
prefix.

### Line

A line keeps a line's trains together. A line still shares track with other lines.
`train_segment_v4` carries a `route_name` for each leg. On 2026-11-14, over the 34,521 sessions
with `realtime_percentage` ≥ 90, experiment 1 found:

- **`route_name` names a train, not a line.** It joins the line and the train number, as in
  `RB71 (19589)`, and takes 23,232 values. The line is the part before ` (`, which takes 409 values.
- **A line's name repeats across operators.** 144 of the 409 names belong to more than one agency.
  `RB41` names six lines, from Bremen to Bavaria. (agency, line) gives 929 pairs.
- **Some legs carry no agency.** 227 sessions from July have none.

Whether each (agency, line) pair draws one line rests on inspecting a map, pair by pair. The key
also rests on how each country's feed names its lines. Both rule the line out as the key.

### Block of space

A block keeps together the sessions that run over the same ground. A whole session falls in the
block of a chosen point, such as its first sample. A long session crosses many blocks, so some of
its track then lies in blocks of the other set.
[Valavi et al.](https://doi.org/10.1111/2041-210X.13107) size blocks from the range over which the
data stays correlated. Here that range is about the distance a train covers within N, up to about
25 km at 300 km/h.

## Splitting by cell of a global grid

A split by cell of a global grid is disjoint by construction, and it needs nothing from a feed but
positions. It therefore works the same in every country. The split runs as follows:

1. A split chooses one resolution. A sample's **chunk** is the cell at that resolution that holds
   it.
2. A **sub-session** is a run of one session's consecutive samples in one chunk.
3. The hash of a salt and each chunk's id assigns the chunk to the tune set or the test set.

The cells at one resolution cover the globe without overlap. Every sample therefore lies in exactly
one chunk, and every chunk in exactly one set. No sample can then fall in both sets.
[blockCV](https://rdrr.io/cran/blockCV/man/cv_spatial.html) splits by hexagonal block by default,
and [spatialsample](https://cloud.r-project.org/web/packages/spatialsample/refman/spatialsample.html)
by a grid of blocks, so a grid of cells follows common practice.

### A5 rather than H3

[A5](https://a5geo.org/) tiles the globe in pentagons, and [H3](https://h3geo.org/) in hexagons.
A5 suits this split better, since its cells at one resolution have equal area. A chunk then covers
the same ground in every country. [H3's cells](https://a5geo.org/examples/area) vary in area by a
factor of about 2 across the globe.

Each grid has the libraries the split needs. A5 has `pya5` 1.0.0 and the Rust crate `a5` 1.0.0.
H3 has `h3` 4.5.0 and the pure-Rust crate `h3o` 0.11.1. Apache Sedona's Spark API has
`ST_H3CellIDs`, which the SedonaDB this repo runs lacks.

Drawing an A5 cell on a map takes two precautions:

- **lonboard 0.17.0's `A5Layer` draws `pya5` 1.0.0's cells in the wrong place.** Chunks over Germany
  appeared over the North Sea, and some routes had no chunk under them. `pya5`'s `migrate` rewrites
  a cell's id, as from `0x63f8a…` to `0x63f4a…`. A change of numbering at A5 1.0 would explain the
  error, though nobody has yet checked lonboard's copy of A5. A polygon from `pya5`'s
  `cell_to_boundary`, drawn in a `PolygonLayer`, lands in the right place.
- **`cell_to_boundary` returns a cell's five corners alone by default.** An A5 cell's edges curve in
  longitude and latitude, so 358 of 326,656 samples fell outside their own cell's drawn outline. With
  `{"segments": 50}`, each edge takes 50 segments, and every sample falls inside.

An A5 cell's area falls by a factor of 4 per resolution:

| Resolution | Area | About |
| --- | --- | --- |
| 7 | 2,075 km² | 45 km across |
| 8 | 519 km² | 23 km across |
| 9 | 130 km² | 11 km across |

### What a sub-session's edges cost

A sub-session starts and ends at a chunk's edge, not at a stop, and that costs the evaluation in
three ways:

- **A predictor needs samples to measure a speed.** Scoring already skips each session's first
  minute, and a sub-session loses that minute again at every chunk it enters.
- **A crossing ahead can lie in the next chunk.** A notification in a sub-session's last N then
  has its pass in another sub-session, perhaps in the other set. It counts as a false alarm.
- **Track runs on across an edge.** The track a tune chunk ends with continues into a test chunk.
  blockCV and spatialsample both drop a buffer around the test set for this reason. A buffer of
  cells one N wide removes it, at the cost of the sessions it drops.

On 2026-11-14, experiment 5 cut 1,000 sessions, picked under seed 1, at three resolutions. It
matched each of their 12,672 passes to its sub-session:

| Pieces | Count | Passes in the piece's first minute | Passes under N after the piece starts |
| --- | --- | --- | --- |
| Whole sessions | 1,000 | 4.7% | 13.7% |
| Resolution 7 | 2,663 | 8.4% | 28.2% |
| Resolution 8 | 4,319 | 12.6% | 44.6% |
| Resolution 9 | 7,690 | 19.4% | 62.9% |

A pass under N after its piece starts has an alert window that reaches into the piece before. Even
at resolution 7, cutting doubles the share of such passes. The cost grows with every finer
resolution.

## Scoring by each crossing's chunk

Another way avoids cutting sessions: replay whole sessions, and score each notification and pass in
the set of its crossing's chunk. It follows a rule that both fields below apply: hold out what a
predictor is scored on, and what tuning reads, but not what the predictor reads as it predicts.

- **Forecasting.** [Evaluation on a rolling forecasting origin](https://otexts.com/fpp3/tscv.html)
  forecasts each target from all the history before it. It scores only the targets in the test
  period, though that history overlaps the training data.
- **Spatial cross-validation.** [Roberts et al.](https://doi.org/10.1111/ecog.02881) and blockCV
  assign each observation, the thing predicted, to a block by its location. A model still reads
  what lies around a test observation.

Each scored unit here belongs to one crossing: a notification, a pass, a false alarm, or a miss. A
crossing lies in exactly one chunk, so each scored unit falls in exactly one set. The predictor
reads whole sessions, so its warm-up and its alert windows match those of a real journey. Cutting
sessions instead changes the task: a real train meets no chunk's edge. The rule holds for any
predictor of events at known places.

The rule needs care in three cases:

- **A predictor that learns as it replays.** Its input becomes its training data, so the test set's
  input leaks. Neither CrowFlies nor LightCone learns as it replays.
- **Tuning near a chunk's edge.** A window in a tune chunk can reach track that runs on into a test
  chunk. Dropping each tune window that ends within N of a test chunk removes that. This buffer
  costs the test set nothing.
- **A predictor that holds its data.** [When a predictor holds its data](#when-a-predictor-holds-its-data)
  sets out why it needs a split by time.

## Whether the sets share a shape of track

LightCone reads how far trains turn, so the tune set and the test set need track of the same shape.
Movement ecology calls a path's departure from a straight line its tortuosity.
[Benhamou](https://doi.org/10.1016/j.jtbi.2004.03.016) recommends the **straightness index** for
oriented paths, such as a train's: the distance between a path's ends over the distance travelled.
It runs from 0 to 1, and its value depends on the path's length.
[trajr](https://doi.org/10.1111/eth.12739) computes it, among other measures.

Experiment 6 measures two things over a window of N from each sample:

- **Straightness**, as above. Every window spans N, so windows compare at one scale.
- **Heading angle**: the angle between a sample's heading, from the sample before it, and the
  bearing to its position N later. LightCone's cone bounds this angle.

A window counts towards a set only when all its samples lie in that set's chunks. A window whose
train travels under 500 m is dwelling, and is left out. The windows overlap, so they are not
independent. The two-sample Kolmogorov–Smirnov statistic serves as an effect size, and its p-value
goes unused.

On 2026-11-14, 1,000 sessions split at resolution 8 gave 217,181 windows within one set. Under the
salt `tune-test-1`:

| | Tune | Test |
| --- | --- | --- |
| Straightness p10 / p50 / p90 | 0.812 / 0.957 / 0.998 | 0.808 / 0.959 / 0.998 |
| Heading angle p10 / p50 / p90 | 1.8° / 16.7° / 58.6° | 1.6° / 16.7° / 60.9° |
| Heading angle p99 | 145.0° | 155.8° |

The Kolmogorov–Smirnov statistic is 0.025 for straightness and 0.010 for heading angle. Twenty other
salts show how much any split by chunk varies:

| Measure | `tune-test-1` | Min over 21 salts | Median | Max |
| --- | --- | --- | --- | --- |
| Straightness KS statistic | 0.025 | 0.012 | 0.026 | 0.075 |
| Heading angle KS statistic | 0.010 | 0.007 | 0.016 | 0.037 |
| Heading angle p90, test minus tune | +2.3° | −4.3° | +0.3° | +3.6° |
| Heading angle p99, test minus tune | +10.8° | −12.9° | +5.7° | +17.2° |

The bulk of each distribution matches under every salt. The p99 does not, under any salt. 2,172
windows turn through 148° or more, the p99, and only 151 chunks hold them. The five busiest chunks
hold 20%, and the chunk centred at 8.81° E, 49.90° N holds 8%. A turn that large is a reversal, as
at a terminus. Where those few chunks fall decides a set's p99.

A cone's angle bounded by the tune set's p99 therefore misses the test set's by up to 17°. The
extremes serve to bound the sweep of angles, and the sweep chooses the angle.

The same 21 salts split the chunks at three resolutions. A finer resolution spreads the reversals
over more chunks. A window that crosses from one set into the other is dropped, though, and a finer
resolution drops more:

| Measure | Resolution 7 | Resolution 8 | Resolution 9 |
| --- | --- | --- | --- |
| Chunks | 196 | 649 | 1,759 |
| Chunks holding a window over the p99 | 103 | 163 | 194 |
| The busiest five chunks' share of those windows | 23% | 18% | 15% |
| Windows kept, median over salts | 93% | 87% | 76% |
| Test share of windows, min to max | 0.23 to 0.39 | 0.22 to 0.36 | 0.21 to 0.28 |
| Heading angle KS statistic, median | 0.023 | 0.016 | 0.016 |
| Heading angle p99, test minus tune, min to max | −14.1° to +12.4° | −12.9° to +17.2° | −9.3° to +13.3° |

No resolution steadies the p99. Its range spans 26° at resolution 7, 30° at 8, and 23° at 9, and
resolution 9 costs a quarter of the windows. The reversals lie at a few places, such as termini, and no grid
spreads them further. The bulk of each distribution matches at every resolution, from 8 on as
closely as at 9.

### Uneven sets

Every key above splits by group, so a set's share of sessions differs from its share of groups. A
busy line or a block around a large hub moves hundreds of sessions at once. The sets therefore come
out uneven, and more so for a coarse key than for a fine one.

## Invariants of a split by cell

On 2026-11-14 the first split by A5 chunk in a notebook went wrong three times. Each time, the
result looked plausible:

- lonboard drew the chunks in the wrong place, as [A5 rather than H3](#a5-rather-than-h3) sets out.
- Pentagons drawn from their corners alone left 358 samples outside their own chunk.
- A merge renumbered the sub-sessions, so 46% of samples took another sub-session's set. The shares
  of each set still came out near 70/30.

A simple invariant caught each error, where inspection had not. A split scored by each crossing's
chunk holds these:

1. **Disjoint.** No chunk lies in both sets.
2. **Complete.** Every scored unit of a run falls in exactly one set. The units of the two sets sum
   to the units of the run.
3. **One chunk per crossing.** Each crossing lies in one chunk, and every scored unit of the
   crossing takes that chunk's set.
4. **One set per tuning window.** A window that tuning reads lies wholly in the tune set's chunks.
5. **Within the country.** Every chunk that holds a crossing or a sample lies in the cells that
   overlap the country's land and maritime areas. `polygon_to_cells` finds those cells with
   `'overlapping'` containment, which shares no code with placing a point in a cell.
6. **Stable.** One salt, resolution, and chunk give one set on every run.

Property tests over generated sessions hold invariants 1 to 4 and 6. A test of 6 states the
expected set as a literal, so a changed hash or salt fails it.

Every run of the evaluation also checks invariants 1, 2, 3, and 5 against the store, and every
tuning step checks 4. A run that fails a check is refused, as a run from uncommitted code is. A
fixture shows the split holds on the fixture, and the check shows it holds on the data the run
scores. The run records the counts each check compared, so its report shows the split was checked.

## Experiments

Each experiment below runs in a notebook against the sessions the filter keeps.

1. **Is `route_name` a line?** Count the sessions per `route_name`, and per agency and `route_name`.
   Draw the paths of a few names on a map, and check that each draws one line.
   [Line](#line) gives the answer, which rules the line out as a key.
2. **How much does each key leak?** Assign sets under each key. For each test session, measure the
   share of its samples that lie within 1 km of a tune session's path. The key with the lowest share
   leaks least. A random split by session gives the baseline.
3. **How uneven are the sets?** Under each key, count each set's sessions and samples, against the
   70/30 aimed for.
4. **Do the sets differ in difficulty?** Run CrowFlies at 1 km over the tune and test sets of each
   key. CrowFlies reads nothing from the sessions, so a gap between its two scores comes from the
   sets alone. A coarse key can put most of a hard region into one set.
5. **What does a chunk's resolution cost?** Split by A5 chunk at resolutions 7, 8, and 9. Count the
   sub-sessions and their lengths, and the share of passes within N of a sub-session's edge.
   [What a sub-session's edges cost](#what-a-sub-sessions-edges-cost) gives the answer.
6. **Do the sets share a shape of track?** Compare straightness and heading angle between the sets,
   under the chosen salt and 20 others.
   [Whether the sets share a shape of track](#whether-the-sets-share-a-shape-of-track) gives the
   answer.
7. **Does cutting sessions change a predictor's score?** Score CrowFlies at 1 km over one set of
   sessions twice: replayed as cut sub-sessions, and replayed whole and scored by each crossing's
   chunk. CrowFlies tunes nothing, so any gap comes from the method alone.
   [Cutting against scoring by chunk](#cutting-against-scoring-by-chunk) gives the answer.

## Cutting against scoring by chunk

On 2026-11-14, experiment 7 ran CrowFlies at 1 km over the test set of 1,000 sessions, split at
resolution 8 under `tune-test-1`. The cut replay read its sub-sessions from a scratch store. Both
divide false alarms by the 266.4 hours spent in test chunks:

| | Cut sub-sessions | Whole sessions, by crossing's chunk |
| --- | --- | --- |
| Sessions replayed | 1,289 sub-sessions | 1,000 |
| Passes scored | 3,294 | 3,625 |
| Notifications | 9,461 | 9,450 |
| False alarms | 5,718 | 5,333 |
| Precision | 0.299 [0.288, 0.310] | 0.328 [0.316, 0.341] |
| Recall | 0.860 [0.838, 0.881] | 0.855 [0.826, 0.880] |
| False alarms per test hour | 21.5 | 20.0 |
| Lead time p10 / p50 / p90 | 20 / 40 / 80 s | 20 / 40 / 90 s |

Cutting lowers precision, and the two intervals do not overlap. It adds 385 false alarms, from
notifications whose pass falls past a chunk's edge. It also scores 331 fewer passes, lost to the
warm-up at each sub-session's start and to sub-sessions of one sample.

Scoring by chunk reproduces CrowFlies' run at 1 km over 12,082 whole sessions: precision 0.323,
recall 0.844, and 19.7 false alarms per hour. Cutting moves the score away from it. Cutting
therefore measures the cuts as well as the predictor, and scoring by chunk measures the predictor
alone.

## The choice

Chosen with the user on 2026-11-14:

- **The key is the A5 cell at resolution 8, about 23 km across.** A split by cell is disjoint by
  construction and needs nothing from a feed but positions, so it works in every country. Resolution
  8 matches the sets' shape of track as closely as resolution 9 does, and keeps 87% of the windows
  within one set rather than 76%. Resolution 7 gives lumpier sets.
- **Sessions replay whole, and each scored unit takes the set of its crossing's chunk.** Cutting
  sessions at chunks' edges lowered CrowFlies' precision from 0.328 to 0.299. Scoring by chunk
  reproduced the score of whole sessions. It holds out what a predictor is scored on, not what it
  reads, as forecasting and spatial cross-validation do.
- **Tuning reads only windows wholly in tune chunks.** A tuning window then reads no test track.
- **The salt is `tune-test-1`, and 70 of 100 buckets go to tune.** The salt's sets fall within the
  spread of 21 salts. Choosing a salt by its sets' scores would tune on the test set.
- **The extremes of the heading angle set only the range of the sweep.** Under every salt and
  resolution, the p99 differs between the sets by up to 17°, since a few places hold most
  reversals. The sweep over the tune set chooses the cone's angle.

The key, the resolution, the salt, and the bounds become settings each run records. Once LightCone
reads the tune set, the split stays fixed for the rest of the slice. A change after that puts track
LightCone was tuned on into its test set.

## When a predictor holds its data

[The observational predictor](2026-10-03-observational-predictor.md) builds a map from (speed,
direction, cell) to the crossings reached, out of recorded journeys. The map holds the network it
was built from. Its claim differs from LightCone's: it predicts later trains over track it has seen,
not trains on track it lacks. This doc's choices need revisiting for it:

- **The key.** A split by block or by line tests the map on cells it never saw, where only its
  back-off answers. A split by time tests what the map claims: build on earlier journeys, and test
  on later ones. Two days under one timetable cannot give that split. A later capture can.
- **The third set.** The map learns a distribution in every bucket, as a model's weights are
  learned. Its resolution, buckets, and back-off are then chosen by sweep. Building and choosing
  each need sessions the other has not read.
- **Drift.** A test set captured weeks after the build measures how fast the map goes stale. That
  doc lists staleness as a weakness to detect.

## References

- [Cross-validation strategies for data with temporal, spatial, hierarchical, or phylogenetic structure](https://doi.org/10.1111/ecog.02881)
  — why a random split overstates performance, and splitting by block
- [Leakage in data mining: formulation, detection, and avoidance](https://doi.org/10.1145/2382577.2382579)
  — leakage, its kinds, and how to find it
- [On over-fitting in model selection and subsequent selection bias in performance evaluation](https://jmlr.org/papers/v11/cawley10a.html)
  — the bias from choosing settings and then testing on the same data
- [Machine Learning Design Patterns](https://www.oreilly.com/library/view/machine-learning-design/9781098115777/),
  chapter 6 — repeatable splitting by a hash of a grouping column
- [blockCV](https://doi.org/10.1111/2041-210X.13107) — sizing spatial blocks from the range of
  autocorrelation
- [blockCV's `cv_spatial`](https://rdrr.io/cran/blockCV/man/cv_spatial.html) and
  [spatialsample](https://cloud.r-project.org/web/packages/spatialsample/refman/spatialsample.html)
  — hexagonal and grid blocks, and a buffer around the test set
- [A5](https://a5geo.org/) — a global grid of equal-area pentagons
- [H3](https://h3geo.org/) and [h3o](https://github.com/HydroniumLabs/h3o) — a global grid of
  hexagons, and its Rust implementation
- [Apache Sedona's `ST_H3CellIDs`](https://sedona.apache.org/1.9.0/api/sql/Spatial-Indexing/ST_H3CellIDs/)
  — covering a geometry with H3 cells in SQL
- [Evaluation on a rolling forecasting origin](https://otexts.com/fpp3/tscv.html) — forecasting
  from all prior history, and scoring only the test period
- [How to reliably estimate the tortuosity of an animal's path](https://doi.org/10.1016/j.jtbi.2004.03.016)
  — the straightness index for oriented paths, and sinuosity for random search
- [trajr](https://doi.org/10.1111/eth.12739) — straightness, sinuosity, and other measures of a
  path
- [scikit-learn: cross-validation iterators for grouped data](https://scikit-learn.org/stable/modules/cross_validation.html#cross-validation-iterators-for-grouped-data)
  — `GroupShuffleSplit` and `GroupKFold`
