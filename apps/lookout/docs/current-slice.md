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

- **A value built from an input already typed comes through `TryFrom` with `type Error = !`.**
  Confirmed 19 November 2026. Rust 1.100 makes `!` nameable anywhere, so the call site reads
  `let Ok(id) = GersId::try_from(uuid);`, and its type states that the call cannot fail. The one
  `expect` left sits inside the impl, with its allow. An allow at every call site loses: it states
  in prose what the type can state. A constant, such as a value in `Row::example`, keeps its allow.

#### Consequences elsewhere

- `crates/server/Cargo.toml` has no `[lints]` table, so the workspace lints miss `server` today.
  Adding `lints.workspace = true` also subjects it to `clippy::all`.
- `m5plus` is a workspace of its own and inherits no lints. Clippy on it needs the ESP-IDF
  toolchain, so the check runs outside the sandbox.

#### Rejected / deferred

- **Keeping the checked form.** `EpisodePath` and `Polyline` re-parse text that construction
  already checked, so each carries an allowed `expect`. Holding the parsed states and line instead
  removes the call. It changes the stored type, so it waits for a refactor of its own.

### Tasks

#### Refactors / extensions

- [x] Add a `just clippy` recipe, so every session runs the same check.
- [x] Add `lints.workspace = true` to `crates/server/Cargo.toml`, and clear what `clippy::all`
      then flags there.

- [x] Upgrade the toolchain to 1.100 in `rust-toolchain.toml`, in a commit of its own.
- [ ] Upgrade `m5plus` to `esp-1.100.0.0` in its `rust-toolchain.toml` and in the Justfile's
      `LIBCLANG_PATH`, in a commit of its own. Installing it runs outside the sandbox.
      `esp-1.99.0.0` failed to compile `std` for ESP-IDF, so the build decides first. The device
      then soaks as for 1.98.1.0. If either fails, `!` stays out of the crates `m5plus` builds.
- [ ] Fold the [Rust 1.100 assessment](2026-11-19-rust-1.100.md) into this slice.
  - [ ] Fold each change listed under "Changes to the plan" into this slice's decisions and tasks.
  - [ ] Add an Opportunities task group before the Wrap-up group. Its tasks apply each change
        listed under "Opportunities in existing code" to the code already written.
- [ ] Give each conversion from a typed input a `TryFrom` with `type Error = !`: `Uuid` to
      `GersId` and to `MotisPolylineId`, and `DateTime<Utc>` to `ExtractId`. Only after the
      upgrade lands. Calls found in later crates join this task.

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
- [ ] Move `countries::fixtures` to a `test-countries` crate, and the `countries` unit tests that
      use it to `crates/countries/tests/`. Point `transport` and `evaluation` at the new crate.
- [ ] Move `predictor::fixtures` to a `test-predictor` crate, and the `predictor` unit tests that
      use it to `crates/predictor/tests/`. Point `platform-core` and `m5-core` at the new crate.
- [ ] `domain` (9) and `recorder` (7).
- [ ] `predictor` (1, once its fixtures move), `motis` (4), and `crossings` (3).
- [ ] The rest, two or fewer each: `platform/web/core`, `platform/web/bridge`, `platform-core`,
      `medallion-py`, `session_crossings`, `server`, `py-common`, and `platform/m5/core`.

#### Binaries, 35 calls

- [ ] `motis` (17).
- [ ] `recorder` (11).
- [ ] `session_crossings` (3), `server` (2), `crossings` (1), and `map` (1).

#### `m5plus`

- [ ] Add `[lints.clippy]` with both lints to `crates/platform/m5/m5plus/Cargo.toml`, and clear
      what it flags. Run clippy outside the sandbox.

#### Wrap-up

- [ ] Raise both lints to `deny`, once clippy flags none, and run `just clippy`.
- [ ] Name the lints, and the three root-level allows, in `.claude/rules/rust.md`.
