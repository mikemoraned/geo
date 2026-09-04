# Testing limits in the sandbox

What a passing test run does and doesn't prove when Claude runs it.

- **No Docker daemon is available under the sandbox**, so `*_docker` tests compile but
  never run there; only `just test-no-docker` is a real signal. `just test` (run outside
  the sandbox) is what actually exercises them.
- **A `_docker` test is often the only end-to-end coverage of a binary's own surface** —
  argument parsing, exit codes, the process actually starting. So "all tests pass" from
  inside the sandbox can still hide a broken CLI. This has bitten once: `--medallion-root`
  was flattened into a parent command, so clap rejected
  `recorder drain --medallion-root X`, and nothing caught it until `just test` ran.
- **When a change touches a binary's arguments or startup, run the binary** —
  `./target/debug/<bin> <args>` — rather than relying on tests alone. A run that fails
  later (on a missing env var, say) still proves the arguments parsed. Add a unit test for
  the parsing itself where one can be written.
- **DuckDB's `INSTALL spatial` fails under the sandbox**: it stages the extension into
  `~/.duckdb/extensions/…` and that write is refused, so anything running
  `INSTALL spatial; LOAD spatial;` dies with an `IOException` naming a `.tmp-…` file. That
  covers the `visualise` Python tests (22 of them error, the other 23 pass) and every
  water-crossings notebook run, so `just test-python` — and therefore `just test-no-docker`
  as a whole — cannot complete here. Run the three Python suites separately to see which
  half is real, and have the user run the notebook recipes.
- **The sandbox writes only inside the app directory**, so a toolchain that installs into
  `$HOME` fails. A device crate therefore keeps its managed ESP-IDF under itself rather than
  in a shared home-directory install, at the cost of several gigabytes per git worktree. The
  failure is a bare "operation not permitted", reported after the whole toolchain resolution
  has run.
- **Nothing in a crate depending on `esp-idf-*` can be run here at all** — it cannot even be
  compiled for the host. A clean device build proves it compiles and links for the target and
  nothing more; behaviour needs a flash, which Claude cannot do. See
  [working-with-claude.md](working-with-claude.md).
- Say plainly, when reporting, which tests could not be run.
