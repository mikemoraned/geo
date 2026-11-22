# Current Slice: refuse `expect` and `unwrap` outside tests

### Target

Make clippy refuse `expect` and `unwrap` outside tests, so that each one left in the code states why
it is there. `.claude/rules/rust.md` asks for this in prose, and no lint enforces it. A text count on
2 November 2026 found about 114 `expect` and 2 `unwrap` calls outside test modules, across 41 files.
37 of them are in `src/bin/`.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 19 November 2026.

- **Clippy's count replaces the text count.** On 19 November 2026, `cargo clippy --workspace
  --all-targets --all-features` with both lints at `warn` and `allow-expect-in-tests` and
  `allow-unwrap-in-tests` set flagged 303 calls, 43 of them `unwrap`. 87 are in library code, 35
  in `src/bin/`, 163 under `tests/`, 13 in `test-redis` and `test-passkeys`, and 5 in `build.rs`
  files. The text count missed the `unwrap` calls and could not tell a test helper from a test.
- **An integration-test crate allows both lints at its root.** `allow-expect-in-tests` covers
  `#[test]` functions and `#[cfg(test)]` modules, not the helpers beside them under `tests/`, such
  as `server/tests/server/support.rs`. A panic is how a test fails, so each `tests/` crate root
  carries one `#![allow(clippy::expect_used, clippy::unwrap_used, reason = "...")]`. Rewriting the
  helpers to return `Result` loses to it: it doubles the diff and changes no outcome.
- **A crate that only tests use allows both lints at its root.** `test-redis` and
  `test-passkeys` take one crate-level allow each, by the same reason.
- **Fixtures sit where a reader sees that only tests use them.** Confirmed 19 November 2026.
  A crate whose fixtures only its own tests use holds them in a `#[cfg(test)]` module, which
  `clippy.toml` already covers. Fixtures shared with other crates' tests move to a test-only crate
  of their own, such as `test-countries`, which takes the crate-level allow above. A
  feature-gated `fixtures` module with an allow of its own loses: it reads as library code. A
  crate's unit tests that use its own shared fixtures move to `tests/`, since a dev-dependency
  on the fixtures crate would compile a second copy of the crate under test, whose types
  differ from the first.
- **A build script allows both lints at its root.** A build script reports failure by
  panicking, as `.claude/rules/rust.md` already says. Each `build.rs` takes one file-level allow
  rather than an allow per call.
- **A binary turns a call into `?` before it allows one.** Where `main` returns a `Result`, a
  call becomes `?` into its boxed error. An allow with a reason remains only for a call that
  cannot fail. Library code follows the Target: a `thiserror` variant where the failure can
  happen, an allow with a reason where it cannot.
- **A call that cannot fail is an `unwrap` under an allow whose reason says why.** Confirmed
  19 November 2026. `#[allow(clippy::unwrap_used, reason = "...")]` states the reason once. An
  `expect` under an allow states it twice, once in the reason and once in the message. Allowing
  `expect_used` everywhere, so that a message justifies the call, loses: Clippy would stop
  flagging a fallible `expect`. The cost is a panic without a message if the reason ever turns
  out wrong, and its file and line lead to the reason.
- **A value built from an input already typed comes through `From`.** Confirmed 19 November 2026.
  The signature then states that the call cannot fail, and the one `expect` left sits inside the
  impl, with its allow. `domain` already does this in `From<Uuid> for DeviceId` and
  `From<Uuid> for SessionId`. A `TryFrom` with `type Error = !` loses: Clippy's
  `infallible_try_from` flags it, and a `From` impl brings a `TryFrom` whose error is `!` anyway.
  A `From` impl also compiles on `esp-1.98.1.0`. `PolylineId::of` and `ExtractId::at` already
  return `Self`, so `Uuid` to `GersId` is the one conversion to add. A constant, such as a value
  in `Row::example`, keeps its allow.
- **A device id holds a UUID.** Confirmed 20 November 2026. The browser mints each id as a UUID,
  and the M5 board sends none, so `DeviceId` holds a `Uuid`, as `GersId` does, and `From<Uuid>`
  cannot fail. The narrower check changes the declarations of `gps_reading_v2`,
  `accel_reading_v2`, `device_session_v2`, `session_v3`, `session_sample_v3`, and
  `session_crossing_v3`, and keeps their versions under the narrowing case
  [medallion.md](medallion.md#dataset-versions) adds. On 20 November 2026 the store held five
  distinct device ids across those datasets, every one a UUID. Keeping a string id with an
  unchecked `From<Uuid>` would have kept the declarations unchanged, but left the id wider than
  what a device sends.
- **A polyline id holds a UUID.** Confirmed 20 November 2026. `PolylineId` already accepted only
  a UUID's text, so holding a `Uuid` accepts the same values, and `PolylineId::of` cannot fail.
  The declarations of `motis_polyline_v2` and `motis_segment_v3` change only in the check's name,
  and keep their versions under the first case [medallion.md](medallion.md#dataset-versions)
  adds. The store's 26,995 polyline ids are all canonical UUID text, so each reads and writes
  back unchanged.
- **A Motis source id and a run id hold a UUID.** Confirmed 20 November 2026. Each is minted as a
  UUID, so `MotisSourceId` and `RunId` hold a `Uuid`, and `From<Uuid>` replaces the `unwrap` in
  `run_id`. The narrower check changes the declarations of `motis_source_v4`, `motis_segment_v3`,
  `motis_poll_v2`, `train_segment_v4`, `train_session_v3`, and `train_session_sample_v3`, and keeps
  their versions under the narrowing case. On 20 November 2026 the store held 8 distinct source
  ids and 33 run ids, every one a UUID. A run id is a partition value, not a column, so no
  declaration records it. `write_gold` in `medallion-py` now refuses a run id that is not a UUID.
- **Generated code allows `unwrap_used` on the module that includes it.** progenitor's Motis client
  calls `unwrap`, and a regenerated file would lose any edit, so `motis::api` carries the allow.
- **The shared crates declare the `rust-version` of the toolchain `m5plus` builds them with.**
  `m5plus` builds `domain`, `predictor`, `platform-core`, and `m5-core`, so a standard library item
  newer than its toolchain breaks the device build. Clippy's `incompatible_msrv`, part of
  `clippy::all`, flags one in those crates. It misses language features such as `!`, so those stay
  out of them by review. On 22 November 2026, `esp-1.100.0.0` built and soaked for 59 minutes
  without a reboot, so the `rust-version` rose from `"1.98"` to `"1.100"`.

#### Consequences elsewhere

- `crates/server/Cargo.toml` has no `[lints]` table, so the workspace lints miss `server` today.
  Adding `lints.workspace = true` also subjects it to `clippy::all`.
- `.claude/memory/working-with-claude.md` says the workspace keeps some unused
  `[workspace.dependencies]` entries on purpose. 6a8e701c removed the last of them, and Cargo
  1.100's `unused_workspace_dependencies` flags any entry that comes back.
- `m5plus` is a workspace of its own and inherits no lints. Clippy on it needs the ESP-IDF
  toolchain, so the check runs outside the sandbox.

#### Rejected / deferred

- **A session that cannot be empty.** `recorder::Session` holds its samples in a public `Vec`, so
  `started_at`, `started_from`, and the silver path each allow an `expect` that a session starts
  from a sample. Only `sessions` builds one, and it always pushes a sample first. A non-empty
  collection, such as the `nonempty` crate's, would make the type say so. It changes a public
  type, so it waits for a refactor of its own.
- **Proving a call free of panics with Kani.** A harness under `cfg(kani)` would give a function
  every input its constructor admits, and Kani would report any input that reaches a panic. It
  adds a tool with a nightly compiler of its own, run apart from `cargo nextest`, and its time on
  the float and geometry code is unmeasured. It returns for a call that neither the build nor
  keeping the checked form can cover, and that a test samples poorly.
- **A `const fn` check for each string newtype.** A newtype's constants, written as text, could
  be checked at build time by a `const fn` taking a `&'static str`, then converted at run time
  without a fallible call. Each type would carry a second validator, written as a byte loop beside
  its real one, and the two can drift apart. URLs and encoded polylines have no reasonable `const`
  validator. Most such constants sit in `example()` functions, which the tests run. It returns for
  a newtype whose constants matter outside tests.
- **Keeping the checked form.** `EpisodePath` and `Polyline` re-parse text that construction
  already checked, so each carries an allowed `expect`. Holding the parsed states and line instead
  removes the call. It changes the stored type, so it waits for a refactor of its own.

### Tasks

#### Refactors / extensions

- [x] Add a `just clippy` recipe, so every session runs the same check.
- [x] Add `lints.workspace = true` to `crates/server/Cargo.toml`, and clear what `clippy::all`
      then flags there.

- [x] Upgrade the toolchain to 1.100 in `rust-toolchain.toml`, in a commit of its own.
- [x] Upgrade `m5plus` to `esp-1.100.0.0` in its `rust-toolchain.toml` and in the Justfile's
      `LIBCLANG_PATH`, in a commit of its own. Installing it runs outside the sandbox.
      `esp-1.99.0.0` failed to compile `std` for ESP-IDF, so the build decides first. The device
      then soaks as for 1.98.1.0. If both pass, raise the shared crates' `rust-version` to
      `"1.100"`. If either fails, it stays at `"1.98"`.
- [x] Fold the Rust 1.100 assessment into this slice.
  - [x] Fold each change listed under "Changes to the plan" into this slice's decisions and tasks.
  - [x] Add an Opportunities task group before the Wrap-up group. Its tasks apply each change
        listed under "Opportunities in existing code" to the code already written.
- [x] Set `rust-version = "1.98"` in the manifests of `domain`, `predictor`, `platform-core`, and
      `m5-core`, and clear what `incompatible_msrv` flags.
- [x] Add `From<Uuid> for GersId`, and use it in `MotisSourceRowV4::example`. Conversions from a
      typed input found in later crates join this task.
- [x] Drop the claim in `.claude/memory/working-with-claude.md` that the workspace keeps unused
      `[workspace.dependencies]` entries on purpose.

#### The lints, at `warn`

- [x] Set `expect_used` and `unwrap_used` to `warn` in `[workspace.lints.clippy]`, and add a
      `clippy.toml` setting `allow-expect-in-tests` and `allow-unwrap-in-tests`. `deny` waits on
      the last group.
- [x] Add the root-level allow to each of the 17 `tests/` crate roots.
- [x] Add the root-level allow to `crates/test-redis` and `crates/test-passkeys`.
- [x] Add the file-level allow to `crates/motis/build.rs` and `crates/platform/m5/core/build.rs`.

#### Library code, 87 calls, 29 of them in fixtures

- [x] `evaluation` (21). Its fixtures became a `#[cfg(test)]` module, and its `fixtures`
      feature went.
- [x] `medallion-model` (19), including `Row::example`, which runs on the write path.
- [x] Move `countries::fixtures` to a `test-countries` crate, and the `countries` unit tests that
      use it to `crates/countries/tests/`. Point `transport` and `evaluation` at the new crate.
      The unit tests stayed: `test-countries` does not depend on `countries`, so no second copy
      compiles.
- [x] Move `predictor::fixtures` to a `test-predictor` crate, and the `predictor` unit tests that
      use it to `crates/predictor/tests/`. Point `platform-core` and `m5-core` at the new crate.
- [x] `domain` (9) and `recorder` (7).
- [x] `predictor` (1, once its fixtures move), `motis` (4), and `crossings` (3).
- [x] The rest, two or fewer each: `platform/web/core`, `platform/web/bridge`, `platform-core`,
      `medallion-py`, `session_crossings`, `server`, `py-common`, and `platform/m5/core`.
- [x] Have `Query::sql_with_params` answer with its schema beside its batches, so `medallion-py`
      reads the schema without an `unwrap`. Drop the empty batch it adds to an empty result.

#### Binaries, 35 calls

- [x] `motis` (17).
- [x] `recorder` (11).
- [x] `session_crossings` (3), `server` (2), `crossings` (1), and `map` (1).

#### `m5plus`

- [x] Add `[lints.clippy]` with both lints to `crates/platform/m5/m5plus/Cargo.toml`, and clear
      what it flags. Run clippy outside the sandbox.

#### Opportunities from Rust 1.100

- [x] Use `Path::trim_prefix` in place of `strip_prefix` and an unwrap, at
      `crates/medallion/src/path.rs:523` and `crates/recorder/tests/sessionise.rs:120`.
- [x] Use `Vec::from_fn` in place of a `usize` range mapped and collected into a `Vec`, at
      `crates/crossings/src/pointset.rs:98`, `crates/crossings/tests/pack.rs:202`,
      `crates/evaluation/src/cone.rs:325`, `crates/evaluation/src/fixtures.rs:59`,
      `crates/evaluation/src/pick.rs` lines 146, 202, 220, and 340,
      `crates/evaluation/src/summarise.rs:427`, and `crates/session_crossings/tests/derive.rs`
      lines 84 and 367. Line numbers hold at 6a8e701c.
- [x] Set `global-min-publish-age = "7 days"` in `apps/lookout/.cargo/config.toml`, and lift it
      in `just audit-update` with `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow`.
- [x] Drop the six `--locked` options from the `prerequisites` recipe.
- [x] Use `str::trim_prefix` at `crates/predictor/src/sentence.rs:47`. Only after the esp upgrade
      passes.

#### Constants checked by the build

- [x] Add a `Radii` type to `predictor`, holding the watching and alerting radii, with a
      `const fn checked` returning `Option<Self>` and a `new` returning `AlertingBeyondWatching`,
      as `HalfWidth` has. Build `DEFAULT_RADII` in a `const` item with an `expect` stating the
      relation. `CrowFlies::new` then takes a `Radii` and cannot fail, and `platform-core`'s
      allowance goes. The refusal's tests move from `CrowFlies` to `Radii`.
- [x] Give `Percentage` a `const fn checked` too, and build the constant percentages in
      `EvaluationRunRowV6::example` and `TrainSessionRowV3::example` through it.
      nutype's `const_fn` makes `try_new` a `const fn`, so `percentage_in_range` became one.
      It accepts the same values, so the declarations keep their versions and record the new
      hash. Each percentage sits in a `const { }` block, which `cargo build` evaluates and
      `cargo check` and `cargo clippy` skip. A `const` item, such as `DEFAULT_RADII`, fails all
      three.
- [x] Replace `holds_points` with a `const fn` in `platform-core`'s `pointset` that makes every
      check `PointSet::new` makes, the coordinates included, reading each with `from_le_bytes`.
      `PointSet::new` calls it before casting, and `m5-core`'s carried set asserts it at build
      time. The `unwrap` in `carried::crossings` then states that the build runs those checks.
- [x] Build the GERS division ids in `medallion-model`'s `overture::division_id` with `uuid!`,
      which parses a literal at build time, and convert them through `GersId::from`. Do the same
      for any other constant that holds a UUID.
      Every other UUID literal sits in a test module. The test that parsed each division id
      went, since the build now parses them.
- [x] Remove `impl Default for MotisClient`, whose allowance claims a check that only the
      end-to-end profile runs. Only tests call it, and each builds its client through
      `MotisClient::new` from `DEFAULT_BASE_URL` instead.

#### Keeping what a constructor checked

- [x] Have `Gap` and `Lead` in `recorder` hold the `u32` of seconds their constructors compute,
      so `as_seconds` returns it rather than unwrapping a second computation. Build the defaults
      from `DEFAULT_GAP` and `DEFAULT_LEAD` without an `unwrap`.
      `Age::seconds` became a `const fn`, so `Gap::new` and `Lead::new` did too.
      `DEFAULT_GAP` and `DEFAULT_LEAD` now hold a `Gap` and a `Lead`, each built in a `const`
      item. `Result::expect` is not `const`, so each matches and panics with its reason.
- [x] Have `Span` in `domain` hold the `Age` its constructor checks, so `time_delta` and `age`
      unwrap nothing, and `Age` the `Duration` that `From<Age> for Span` needs. Lower priority
      than `Gap` and `Lead`: do it only if `Span::minutes` stays a `const fn` without an `unwrap`.
      `Age::new` converts its span with `to_std`, which serves as its check. `Span::new` and
      `TryFrom<TimeDelta>` then refuse through `Age::new`.

#### Wrap-up

- [x] Add `allow_attributes` and `allow_attributes_without_reason` to `[workspace.lints.clippy]`
      and to `m5plus`'s `[lints.clippy]`, and turn each outer `#[allow]` into an `#[expect]`.
      Delete the allowance on `GpsReadingRowV2::example`, which covers nothing since `DeviceId`
      holds a `Uuid`. `motis::api` keeps its `#[allow]`, since what it suppresses depends on what
      progenitor generates. Do this before raising the lints to `deny`.
      progenitor's output carries `#[allow]`s of its own, so `motis::api`'s `#[allow]` names both
      new lints too. Every other `#[expect]` suppresses a warning, so no other allowance had gone
      stale. The new lints flag nothing in `m5plus`.
- [x] Fold what holds from `docs/2026-11-22-infallible-calls.md` into `.claude/rules/rust.md`: a
      constant's `expect` in a `const` item or block, `#[expect]` in place of `#[allow]`, and a
      constructor that keeps what it computed. Then delete the doc.
- [x] Raise both lints to `deny`, once clippy flags none, and run `just clippy`. It prints
      Cargo's warnings beside Clippy's, and the run clears both.
      With the two lints at `deny`, `clippy::all` at `warn` shared their priority, so Clippy's
      `lint_groups_priority` warned. `all` now takes priority `-1`.
- [x] Name the lints, and the three root-level allows, in `.claude/rules/rust.md`.
- [x] Delete `docs/2026-11-19-rust-1.100.md`. Done before its tasks, once each carried what it
      needed from the assessment.
