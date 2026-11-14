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

`trip_id` keeps one trip's two days together. It leaves the 10:00 and 10:20 trains of one line in
different sets, so it removes the least leakage. Every session carries it.

### Line

A line keeps a line's trains together. `train_segment_v4` carries a `route_name` for each leg,
which nobody has yet checked as the name of a line. A name such as `RE 1` can repeat across regions,
so experiment 1 checks whether the key needs the agency beside it. A line still shares track with
other lines.

### Block of space

A block keeps together the sessions that run over the same ground. A session falls in the block of
a chosen point, such as its first sample or the centre of its bbox. A long session crosses many
blocks, so some of its track lies in blocks of the other set.
[Valavi et al.](https://doi.org/10.1111/2041-210X.13107) size blocks from the range over which the
data stays correlated. Here that range is about the distance a train covers within N, up to about
25 km at 300 km/h.

### Uneven sets

Every key above splits by group, so a set's share of sessions differs from its share of groups. A
busy line or a block around a large hub moves hundreds of sessions at once. The sets therefore come
out uneven, and more so for a coarse key than for a fine one.

## Experiments

Each experiment below runs in a notebook against the sessions the filter keeps.

1. **Is `route_name` a line?** Count the sessions per `route_name`, and per agency and `route_name`.
   Draw the paths of a few names on a map, and check that each draws one line.
2. **How much does each key leak?** Assign sets under each key. For each test session, measure the
   share of its samples that lie within 1 km of a tune session's path. The key with the lowest share
   leaks least. A random split by session gives the baseline.
3. **How uneven are the sets?** Under each key, count each set's sessions and samples, against the
   70/30 aimed for.
4. **Do the sets differ in difficulty?** Run CrowFlies at 1 km over the tune and test sets of each
   key. CrowFlies reads nothing from the sessions, so a gap between its two scores comes from the
   sets alone. A coarse key can put most of a hard region into one set.

## What the choice decides

The group key, the salt, and the bounds of each set become settings `pick` records. Once LightCone
reads the tune set, the split stays fixed for the rest of the slice. A change after that puts
sessions LightCone was tuned on into its test set.

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
- [scikit-learn: cross-validation iterators for grouped data](https://scikit-learn.org/stable/modules/cross_validation.html#cross-validation-iterators-for-grouped-data)
  — `GroupShuffleSplit` and `GroupKFold`
