# Current Slice: Evaluation framework with associated Crow Flies predictor improvement

### Target

I have some larger ideas for how to come up with a good predictor, but for now I want to prove I can improve things + measure that improvement in some way.

So, I'd like to go from a very simple Crow Flies predictor I have now, which effectively just shows distance to nearby crossings, to one that takes into account current speed and average direction, and predicts when a crossing will be passed in next N minutes.

### Straw Man

This approach means we have to become a lot crisper in what a prediction means. Right now the display just shows a radar style view which isn't really making a prediction and is instead just a map of nearby crossings. My thinking here is that, for each crossing, we need to have a little state-machine, something like:

States:
* OutOfReach: this is maybe an implicit state that any crossing sits in, if it is just in the pointset data, and isn't reachable at all
* CandidatePass: this is when it becomes nearby enough to be considered as a possible crossing
* PredictedPass: this is when we expect we will actually pass by this crossing in the next N minutes
* ActualPass: we just actually passed the crossing

Transitions:
* OutOfReach <-> CandidatePass: a crossing can oscillate in and out of being a candidate
* OutOfReach -> PredictedPass or CandidatePass -> PredictedPass: this is us making a prediction, and this transition should lead to a notification of some kind to the user
* PredictedPass -> CandidatePass or PredictedPass -> OutOfReach: this counts as a failed prediction
* PredictedPass -> ActualPass: a successful prediction. Note that a crossing stays in the ActualPass state until we've gotten some distance away from it to avoid re-notifying repeatedly. In general, the state transitions should probably have some sort of debouncing to avoid frequent flips in and out of states.
* ActualPass -> PredictedPass: it's always possible we can pass somewhere multiple times, so this is a new prediction
* ActualPass -> CandidatePass: still within reach but not currently predicted
* ActualPass -> OutOfReach: it's now distance from us and we don't even think it is a candidate

In this framework, depending on how clever the predictor is, we may be able to display a crossing but still label it as OutOfReach. This could be, for example, if we decide it is technically close by but we are not going to reach it because it's not on our current track.

However, the cleverness of the predictor we do in this slice should be limited to using a current-speed/direction + light-cone approach i.e. predict what we will reach based on where we will pass by based on current vector. This can start with a low-level of cleverness about current speed/direction (i.e. just literal current speed) and then start building up an overall speed/direction based on an average over last few fixes and/or on speed reported by the GPS device. 

I suspect on Web we could benefit from the cleverness of the GPS baked into the phone, as it's probably doing something clever, but I'd prefer not to use that and instead do it ourselves, as then we have consistent performance across platforms. I am thinking here of some sort of Kalman-filter or similar.

We should implement an evaluation framework which uses advice from apps/lookout/docs/2026-08-01-evaluation.md and applies it to saved sessions from myself (silver/session table) and from motis (bronze/motis_segment). The idea is to use real recorded data from being on a train or from reported positions of trains to drive an evaluation of what the predictor says about future water crossings compared to when they actually happened. We can use silver/session_crossing for this, and we may want to apply the same pattern to motis data i.e. treat motis train tracking as a session.

We should get new motis data by polling motis live in a particular bbox and watching when trains arrive. The idea is that we need to get enough data that we can put together a reasonable size test dataset, and *also* that we gather enough data to do more ambitious stuff with it later, where we use the motis data as effectively input data for a model or a dataset.

If we use transitious.org then we can benefit from more accurate paths (see pfaedle slice), but we should be good citizens and not spam it constantly. We also *probably* don't need this level of route accuracy for a first-cut of a prediction framework. However, it means we should be sure to explicitly model the source of our motis data with metadata about e.g. as a small table in bronze which records which motis version/setup was used and which the actual motis samples in bronze can have a foreign key to. This allows us to ignore sources as needed when building the canonical datasets in Silver.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

- **An upgrade phase comes first: geo, then crux.** The slice changes the predictor, which uses
  geo, and the crux core, which will send notifications. Upgrading first keeps each upgrade's
  breakage apart from the slice's own work, as the upgrades rule asks.
- **geo moves to 0.33.1, ahead of SedonaDB.** sedona-geo at 0.4.1 resolves geo 0.31, so the host
  tree carries both versions until the SedonaDB 0.5.0 slice. The device tree holds no SedonaDB,
  and carries 0.33.1 alone. Waiting for that slice keeps one geo version, but holds the predictor
  on the older geo through this slice.
- **crux_core moves to 0.20.0, the latest.** The reboot loop of 2026-09-21 struck on 0.16.2 as
  well, so the pin does not prevent reboots. The 0.20 fault in `docs/device.md` was seen on the
  1.90.0 ESP toolchain, and the device has not run 0.20 on esp-1.98.1.0. Newer crux brings the
  framework features that notifications want.
- **A soak before and after the crux move measures its effect on stability.** The baseline soak
  runs after the geo move, so the two soaks differ by crux alone. The soak of 2026-10-03 predates
  the geo move, so it does not serve as the baseline. Each soak runs 30 minutes, the length of the
  clean soaks on record, under the same conditions. Each records reboots, unused stack, free heap,
  and the slowest sentence's time, as the soak of 2026-10-03 did.

#### Consequences elsewhere

- `docs/device.md` heads a section "`crux_core` after 0.16.2 reboots the device", and tells a
  reader to pin `=0.16.2`. The comment over the pin in `crates/platform/web/core/Cargo.toml` says
  0.20 reboots the board. Both change with the second soak.
- The workspace `Cargo.toml` comment pins geo to the version sedona-geo resolves.
- The SedonaDB 0.5.0 slice in `docs/next-slices.md` moves geo to 0.33 itself, and that task
  becomes moot.
- `App` in `crates/platform-core/src/app.rs` declares `type Capabilities = ()` and takes a `_caps`
  argument in `update`. crux removed both after 0.16.2.

#### Open questions

- Whether crux 0.20 changes more of `App` and the shells than `Capabilities` and `caps`. Three
  minor releases separate 0.16.2 from 0.20.0. The first build after the move answers it.
- What follows if crux 0.20 reboots the device. A revert to 0.16.2 keeps the slice moving.
  `docs/device.md` also lists bisecting 0.17 to 0.19, at one flash and a 15-minute soak each.

### Tasks

#### Upgrades

Each upgrade lands in a commit of its own, before the slice's own work starts.

- [ ] Move `geo` to 0.33.1 in the workspace `Cargo.toml`, `crates/domain`, and
      `crates/predictor`. Rewrite the workspace comment on its pin.
- [ ] Run `just test-no-docker` and `just test-geo`, and build with `just m5plus-build-release`.
- [ ] Flash the release build and soak it for 30 minutes, as the baseline. The user runs this.
- [ ] Move `crux_core` to `=0.20.0` in `platform-core`, `platform/web/core`, `platform/web/bridge`,
      `platform/m5/core`, and `platform/m5/m5plus`.
- [ ] Drop `type Capabilities` and the `_caps` argument from `App` in
      `crates/platform-core/src/app.rs`.
- [ ] Run `just test-no-docker`, and build with `just wasm` and `just m5plus-build-release`.
- [ ] Flash the release build and soak it for 30 minutes under the baseline's conditions. The
      user runs this.
- [ ] Rewrite the crux section of `docs/device.md` from the two soaks, and the comment over the
      pin in `crates/platform/web/core/Cargo.toml`.
- [ ] Mark the geo task moot in the SedonaDB 0.5.0 slice in `docs/next-slices.md`.

#### Still to decompose

...
- [ ] Delete `docs/2026-08-01-evaluation.md` at the end of this slice. It is a dated
      assessment of how to measure a predictor, written before one existed and before there
      was any ground truth to measure against, and kept for the history of the decision.
      Anything in it still holding by then belongs in the tasks above, in the notebooks
      that implement the measures, or in a durable doc alongside `medallion.md`; the rest —
      the rejected alternatives, the reasoning about metrics from other domains — goes
      stale once a first run has actually produced numbers.
