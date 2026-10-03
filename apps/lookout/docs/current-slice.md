# Current Slice: upgrades

### Target

Pinned versions are how this repo stays reproducible, and the cost is that they age quietly
until something forces a move. The move is then taken mid-slice, under pressure, with no way to
tell an upgrade's breakage from the day's work: marimo went 0.23.15 to 0.25.0 that way, in the
middle of the UK slice, because a sandbox stopped resolving. This slice is the scheduled version
of that — find what is behind, move it deliberately, and record what each move cost.

One upgrade is owed: SedonaDB 0.4.1, the latest release. It was expected to fix a panic, and
it does not.

**SedonaDB panics scanning a bronze partition with nested columns ahead of its geometry.**
`index out of bounds: the len is 7 but the index is 18`. The row-group pruning collects geometry
statistics for the unnested columns alone, then looks them up by the geometry's position among
all of them. `transportation/segment` carries geometry at index 18 behind several nested columns,
and panics. `divisions/division_area` carries it at index 1, ahead of any nested column, and reads.
Issue [#389](https://github.com/apache/sedona-db/issues/389) has the same symptom and a different
cause. Every release so far panics, 0.5.0-rc0 included.
[2026-10-03-sedona-nested-column-panic.md](2026-10-03-sedona-nested-column-panic.md) has the cause,
the smallest reproduction, and the possible actions. The workaround stays. A reader takes bronze
geometry as plain parquet and decodes it from WKB. A fixture mirroring a release selects geometry
ahead of its nested columns. The [medallion README](../crates/medallion/README.md#geometry-a-scan-cannot-read)
describes both.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-02.

- **SedonaDB moves to 0.4.1, and no further.** 0.4.1 builds on the same arrow 57.1 and
  datafusion 52.5 as 0.4.0, so its commit changes SedonaDB alone. Narrowed on 2026-10-03, from a
  move to 0.4.1 and then 0.5.0: neither fixes the panic. The 0.5.0 move and the arrow generation
  it brings are in `next-slices.md`.
- **No one has shown that 0.4.1 cures the panic, so the scan comes first.** Issue #389 was closed
  on 2026-08-07 with no linked commit. PR #385 merged in December 2025, before 0.4.0, and 0.4.0
  still panics, so #385 is a partial fix rather than the cure the Target credits it as. The
  nearest change in 0.4.1 is PR #1116, which changes how the GeoParquet reader handles
  projection expressions. The scan showed that 0.4.1 still panics.
- **The report is a `just outdated` recipe over tools already installed.** `cargo upgrade
  --dry-run` covers the crates.io dependencies, `uv tree --outdated` each uv project,
  `git ls-remote` the SedonaDB tags, and the esp-rs/rust-build releases the ESP toolchain. No
  cargo tool sees a git-pinned dependency, so SedonaDB needs its own check whichever tool runs
  the rest. cargo-outdated reports more, but it adds a prerequisite and still misses SedonaDB.
- **The ESP toolchain moves to the latest esp-rs/rust-build release, and is pinned by name.**
  `espup install --toolchain-version <version> --name esp-<version>` installs it under a name
  of its own, and `rust-toolchain.toml` names that channel instead of `esp`. The alternative,
  pinning the 1.90.0 nightly installed now, keeps the reboot soaks' evidence valid but puts off
  the move. A pin is trusted only after a flash and a soak, which Claude cannot run.

#### Consequences elsewhere

- `LIBCLANG_PATH` in the `Justfile` globs `~/.rustup/toolchains/esp/`, so it follows the
  toolchain's new name.
- `docs/device.md` names the `esp` channel under Toolchain, and gives no install command.
- espup writes to `~/.rustup` and `~/.espressif`, which the sandbox grants read-only, so the
  install runs outside it.

#### Open questions

- A new ESP toolchain changes a variable the reboot investigation in
  [2026-09-21-m5-reboots.md](2026-09-21-m5-reboots.md) depends on. A reboot after the move no
  longer separates the toolchain from the core's second effect. Does the soak on the new
  toolchain wait for that fault to be found?
  Moot for this slice: the soak on 1.98.1.0 ran 33 minutes with no reboot, so there was no
  reboot to attribute.

### Tasks

#### The missing writing skill

- [x] Find where `technical-writing:technical-writing` came from: the plugin that provided it,
      and whether `.claude/settings.json` ever enabled it.
      It is the `technical-writing` plugin from the `rnorth/technical-writing` marketplace, which
      the user settings register. `.claude/settings.json` has enabled it since e44e8989.
- [ ] If that plugin still exists, install it and enable it in `.claude/settings.json`.
      Otherwise remove every mention of the skill from `CLAUDE.md`, `tools/prose-gate`, and the
      memory notes. Either way the change lands at the repo root, which the sandbox leaves
      read-only.
      The plugin exists, is installed, and is enabled. The session that decomposed this slice
      started at the repo root and never listed the skill. The next session, started the same
      way, lists it. `~/.claude/settings.json` changed six seconds before the skill was refused,
      and no person edited it. The cause is not yet known.

#### The report

- [x] Add a `just outdated` recipe: `cargo upgrade --dry-run` for the workspace and for
      `crates/platform/m5/m5plus`, `uv tree --outdated` in each uv project, the newest
      SedonaDB tag against the pin in `Cargo.toml`, and the newest esp-rs/rust-build release
      against the m5plus `rust-toolchain.toml`.
      `cargo upgrade` needs `--incompatible --pinned` as well. Without them it lists only
      semver-compatible moves, and hides arrow 60, datafusion 55, and every `=` pin.
- [x] Time a run of `just outdated`, and decide from it how often an upgrade is considered.
      Record the cadence in `README.md`.
      The cadence is an instruction to Claude rather than a fact about lookout, so it is the rule
      `.claude/rules/lookout-upgrades.md`, scoped to `apps/lookout`.

#### SedonaDB 0.4.1

- [x] Move `sedona` and `sedona-geoparquet` to the tag `apache-sedona-db-0.4.1` in `Cargo.toml`.
- [x] Scan `transportation/segment` through `Query::register_at`, selecting `geometry`, to see
      whether the panic is gone. The rest of this group waits on the answer.
      The panic remains, in 0.4.1 and in 0.5.0-rc0, and upstream main has the same code. Its
      cause is not #389, and no upstream issue reports it. See [Investigation](#investigation).
- [-] Read `segment` through `register_at` in `crates/crossings/tests/geo.rs`, selecting
      `ST_AsBinary(geometry)` as `CountryAreas` in `crates/transport/src/countries.rs` does.
- [-] Delete `Query::register_at_without_geometry` once nothing calls it. Keep
      `register_without_geometry`: it reads a partition value, and has no geometry to declare.
- [-] Put geometry where a release puts it in `mirror_holding_one_row_of_each_type`, in
      `crates/transport/src/extract.rs`, and delete the comment citing #389.
- [-] Look for other fixtures that select geometry first to avoid the panic. The comment in
      `extract.rs` is the only one that says so.
      Moot, these four: no release fixes the panic, so the workaround stays. The `extract.rs`
      comment now cites the medallion README instead of #389.
- [x] Run `just test-geo`.
      It passes on 0.4.1 with the workaround still in place.

#### ESP toolchain

- [x] Upgrade espup to its latest release. The user runs this, outside the sandbox.
      espup 0.18.0, installed without `--locked`: its lockfile pins `yoke-derive` 0.8.3, which is
      yanked, and a fresh resolve takes 0.8.4.
- [x] Install the latest esp-rs/rust-build release with espup under the name
      `esp-<version>`. The user runs this, outside the sandbox.
      1.98.1.0, not the latest. 1.99.0.0 installs, but its `std` does not compile for ESP-IDF.
      The slice takes the newest release that builds unchanged, and drops 1.99.0.0 without
      investigating it. `docs/device.md` records the failure.
- [x] Name that channel in `crates/platform/m5/m5plus/rust-toolchain.toml`, and in the
      `LIBCLANG_PATH` glob in the `Justfile`.
- [x] Build with `just m5plus-build-release`.
      It builds with one warning that 1.90.0.0 does not raise: the `linker_messages` lint
      reports what `ldproxy` prints to stderr.
- [x] Flash and soak the device on the new toolchain. The user runs this.
      Soaked on 2026-10-03 for 33 minutes, with no reboot. The reboot loop strikes within two
      minutes of boot, and the clean soaks on record ran 30 minutes. Over the last 19 minutes,
      unused stack held at 10,020 bytes and free heap at 2,276,640 bytes. The slowest sentence,
      the one that scanned, took 8,330–8,664 µs.
- [x] Give the pinned name and the espup command under Toolchain in `docs/device.md`.

#### Wrap-up

- [ ] Say in each upgrade's commit what it cost: what broke, what was rewritten, and what a
      reader would otherwise mistake for the feature it travelled with.
- [ ] Run `just test-no-docker` before every commit, in every group. The user runs `just test`
      for the Docker tests and the notebook recipes.
- [ ] Decide whether to report the SedonaDB panic upstream, from the possible actions in
      [2026-10-03-sedona-nested-column-panic.md](2026-10-03-sedona-nested-column-panic.md).

### Investigation

#### The SedonaDB panic, 2026-10-03

[2026-10-03-sedona-nested-column-panic.md](2026-10-03-sedona-nested-column-panic.md) records the
cause, the versions checked, and the 0.5.0-rc0 build.

The fixture in `mirror_holding_one_row_of_each_type` reproduces the panic without the store. With
its segment select reordered to `id, subtype, class, connectors, geometry, bbox`, the extract test
panics with `the len is 4 but the index is 4`. `connectors` is the one nested column ahead of the
geometry. The water and connector selects put geometry ahead of `bbox`, and they read.
