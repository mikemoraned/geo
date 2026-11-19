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
- **A build script allows both lints at its root.** A build script reports failure by
  panicking, as `.claude/rules/rust.md` already says. Each `build.rs` takes one file-level allow
  rather than an allow per call.
- **A binary turns a call into `?` before it allows one.** Where `main` returns a `Result`, a
  call becomes `?` into its boxed error. An allow with a reason remains only for a call that
  cannot fail. Library code follows the Target: a `thiserror` variant where the failure can
  happen, an allow with a reason where it cannot.

#### Consequences elsewhere

- `crates/server/Cargo.toml` has no `[lints]` table, so the workspace lints miss `server` today.
  Adding `lints.workspace = true` also subjects it to `clippy::all`.
- `m5plus` is a workspace of its own and inherits no lints. Clippy on it needs the ESP-IDF
  toolchain, so the check runs outside the sandbox.

### Tasks

#### Refactors / extensions

- [x] Add a `just clippy` recipe, so every session runs the same check.
- [x] Add `lints.workspace = true` to `crates/server/Cargo.toml`, and clear what `clippy::all`
      then flags there.

#### The lints, at `warn`

- [x] Set `expect_used` and `unwrap_used` to `warn` in `[workspace.lints.clippy]`, and add a
      `clippy.toml` setting `allow-expect-in-tests` and `allow-unwrap-in-tests`. `deny` waits on
      the last group.
- [x] Add the root-level allow to each of the 17 `tests/` crate roots.
- [x] Add the root-level allow to `crates/test-redis` and `crates/test-passkeys`.
- [x] Add the file-level allow to `crates/motis/build.rs` and `crates/platform/m5/core/build.rs`.

#### Library code, 87 calls

- [ ] `evaluation` (21).
- [ ] `medallion-model` (19), including `Row::example`, which runs on the write path.
- [ ] `domain` (9), `recorder` (7), and `countries` (7).
- [ ] `predictor` (5), `motis` (4), and `crossings` (3).
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
