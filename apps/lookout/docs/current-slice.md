# Current Slice: refuse `expect` and `unwrap` outside tests

### Target

Make clippy refuse `expect` and `unwrap` outside tests, so that each one left in the code states why
it is there. `.claude/rules/rust.md` asks for this in prose, and no lint enforces it. A text count on
2 November 2026 found about 114 `expect` and 2 `unwrap` calls outside test modules, across 41 files.
37 of them are in `src/bin/`.

### Tasks

- [ ] Set `expect_used` and `unwrap_used` to `deny` in `[workspace.lints.clippy]`, and allow both in
      tests through a `clippy.toml`. Check which test helpers outside `#[test]` and `#[cfg(test)]`
      the lints still flag.
- [ ] Go through each flagged call in library code. Return an error where the failure can happen.
      Otherwise, add `#[allow(clippy::expect_used, reason = "...")]`, whose reason says why it
      cannot fail. This covers `Row::example`, which runs on the write path.
- [ ] Go through each flagged call in `src/bin/` by the same test.
- [ ] Apply the same lints to the `m5plus` workspace.
- [ ] Name the lints in `.claude/rules/rust.md`.
