# Current Slice: upgrades

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
parquet and decoded from WKB, which is what `Query::register_at_without_geometry` is for. The same
panic reaches a test that writes its own GeoParquet: a fixture mirroring a release selects geometry
as its first column so the scan survives.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-02.

- **SedonaDB moves in two steps: 0.4.1, then 0.5.0.** 0.4.1 builds on the same arrow 57.1 and
  datafusion 52.5 as 0.4.0, so its commit changes SedonaDB alone and shows what the panic fix
  cost. 0.5.0 moves arrow and parquet to 58.3, datafusion to 54.1, and object_store to 0.13. One
  move to 0.5.0 would put the panic fix and that breakage in the same commit.
- **The 0.5.0 step waits for a release.** 0.5.0-rc0 was tagged on 2026-10-02. A pin to a release
  candidate gives up the reproducibility the pin exists for.
- **arrow 58 moves the geo crates with it.** geoparquet, geoarrow-schema, and geoarrow-array go
  to 0.8, pyo3-arrow to 0.17, and serde_arrow to its `arrow-58` feature. None of them builds
  against arrow 58 alone, so they share one commit. The workspace comment in `Cargo.toml` is
  wrong that geoparquet 0.8 moved to arrow 59: 0.8 builds on 58.
- **No one has shown that 0.4.1 cures the panic, so the scan comes first.** Issue #389 was closed
  on 2026-08-07 with no linked commit. PR #385 merged in December 2025, before 0.4.0, and 0.4.0
  still panics, so #385 is a partial fix rather than the cure the Target credits it as. The
  nearest change in 0.4.1 is PR #1116, which changes how the GeoParquet reader handles
  projection expressions.
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

- If 0.4.1 still panics, does the slice try 0.5.0-rc0 to see whether the fix is there, and
  report upstream? The answer decides whether the plain-parquet read and the reordered fixture
  go in this slice or wait for 0.5.0.
- If 0.5.0 is not released while this slice runs, its group goes back to `next-slices.md`.
- A new ESP toolchain changes a variable the reboot investigation in
  [2026-09-21-m5-reboots.md](2026-09-21-m5-reboots.md) depends on. A reboot after the move no
  longer separates the toolchain from the core's second effect. Does the soak on the new
  toolchain wait for that fault to be found?

### Tasks

#### The missing writing skill

- [ ] Find where `technical-writing:technical-writing` came from: the plugin that provided it,
      and whether `.claude/settings.json` ever enabled it.
- [ ] If that plugin still exists, install it and enable it in `.claude/settings.json`.
      Otherwise remove every mention of the skill from `CLAUDE.md`, `tools/prose-gate`, and the
      memory notes. Either way the change lands at the repo root, which the sandbox leaves
      read-only.

#### The report

- [ ] Add a `just outdated` recipe: `cargo upgrade --dry-run` for the workspace and for
      `crates/platform/m5/m5plus`, `uv tree --outdated` in each uv project, the newest
      SedonaDB tag against the pin in `Cargo.toml`, and the newest esp-rs/rust-build release
      against the m5plus `rust-toolchain.toml`.
- [ ] Time a run of `just outdated`, and decide from it how often an upgrade is considered.
      Record the cadence in `README.md`.

#### SedonaDB 0.4.1

- [ ] Move `sedona` and `sedona-geoparquet` to the tag `apache-sedona-db-0.4.1` in `Cargo.toml`.
- [ ] Scan `transportation/segment` through `Query::register_at`, selecting `geometry`, to see
      whether the panic is gone. The rest of this group waits on the answer.
- [ ] Read `segment` through `register_at` in `crates/crossings/tests/geo.rs`, selecting
      `ST_AsBinary(geometry)` as `CountryAreas` in `crates/transport/src/countries.rs` does.
- [ ] Delete `Query::register_at_without_geometry` once nothing calls it. Keep
      `register_without_geometry`: it reads a partition value, and has no geometry to declare.
- [ ] Put geometry where a release puts it in `mirror_holding_one_row_of_each_type`, in
      `crates/transport/src/extract.rs`, and delete the comment citing #389.
- [ ] Look for other fixtures that select geometry first to avoid the panic. The comment in
      `extract.rs` is the only one that says so.
- [ ] Run `just test-geo`.

#### ESP toolchain

- [ ] Install the latest esp-rs/rust-build release with espup under the name
      `esp-<version>`. The user runs this, outside the sandbox.
- [ ] Name that channel in `crates/platform/m5/m5plus/rust-toolchain.toml`, and in the
      `LIBCLANG_PATH` glob in the `Justfile`.
- [ ] Build with `just m5plus-build-release`.
- [ ] Flash and soak the device on the new toolchain. The user runs this.
- [ ] Give the pinned name and the espup command under Toolchain in `docs/device.md`.

#### SedonaDB 0.5.0 and the arrow generation

- [ ] Only once 0.5.0 is released: move `sedona` and `sedona-geoparquet` to its tag, `arrow`
      and `parquet` to 58.3, `datafusion` to 54.1, and `object_store` to 0.13.
- [ ] In the same commit, move `geoparquet`, `geoarrow-schema`, and `geoarrow-array` to 0.8,
      `pyo3-arrow` to 0.17, and `serde_arrow` to its `arrow-58` feature.
- [ ] Move `geo` to the version `sedona-geo` resolves at 0.5.0, as its comment in `Cargo.toml`
      requires.
- [ ] Rewrite the arrow comment in `Cargo.toml` for arrow 58, dropping its claim about
      geoparquet 0.8.
- [ ] Run `just test-geo`.

#### Wrap-up

- [ ] Say in each upgrade's commit what it cost: what broke, what was rewritten, and what a
      reader would otherwise mistake for the feature it travelled with.
- [ ] Run `just test-no-docker` before every commit, in every group. The user runs `just test`
      for the Docker tests and the notebook recipes.
