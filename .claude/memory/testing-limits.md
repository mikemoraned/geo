# Testing limits in the sandbox

What a passing test run does and doesn't prove when Claude runs it.

- **No Docker daemon is available under the sandbox**, so `*_docker` tests compile but
  never run there; only `just test-no-docker` is a real signal. `just test` (run outside
  the sandbox) is what actually exercises them.
- **Mutation testing in the sandbox runs `just mutants-on-diff-no-docker`.** A mutant in code
  that only `_docker` tests cover always survives there, since none of its tests run. Leave such
  files out with `-e <path>`, and have the user run them with `just mutants-on-diff -f <path>`.
- **Run mutants with `--in-place` while free disk is short.** By default cargo-mutants copies the
  workspace and builds the copy from scratch. `--in-place` reuses `target/` instead, but a run cut
  short leaves a mutated file behind, so check `git status` afterwards. An in-place run also leaves
  a `proptest-regressions/` directory beside any crate whose proptests a mutant failed. Its seeds
  record the mutant's failures, not the code's, so delete it.
- **Commit before the user starts an in-place run.** The user watches `git diff` to see each
  mutation, so the tree has to hold nothing else. Leave the code alone until the run reports.
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
  covers every water-crossings notebook run, so have the user run the notebook recipes. The
  four suites in `just test-python` pass here, and so does the rest of `just test-no-docker`.
- **`wasm-pack` writes to `~/Library/Caches` and is refused**, so `just wasm` dies before it
  compiles anything. wasm-pack 0.15 reports it as a bare "File exists (os error 17)", and older
  releases as "Operation not permitted". It caches the `wasm-bindgen` and `wasm-opt` it downloads
  there. Set `WASM_PACK_CACHE` to a scratch dir in the same command, and the first run fetches the
  tools again into it. What the built wasm does can be checked under node without a browser:
  import the `--target web` module and pass the `.wasm` bytes to `init` as `{ module_or_path }`.
- **The sandbox denies the Motis server the mmap of its LMDB tile database**, so the server runs
  outside it, started by the user. Any check against it, such as a realtime A/B, waits for that.
- **The sandbox denies by default and grants only what it is given**, so a path outside the
  app directory is unreachable — for reading as much as for writing — unless the launcher
  lists it. That `~/.cargo` and `~/.rustup` are readable says nothing about any other home
  directory path: they are reachable because they are granted, not because they are outside
  the repo. Grant one by adding it to `--add-dirs-ro` in the root `Justfile`'s `claude`
  recipe, which takes a colon-separated list.
- **A grant added there reaches only a session started afterwards.** The policy is fixed when
  Claude launches, so a change to it cannot be tested in the session that made it.
- The failure either way is a bare "operation not permitted" naming a path that plainly
  exists, and a toolchain reports it only after its whole resolution has run.
- **Nothing in a crate depending on `esp-idf-*` can be run here at all** — it cannot even be
  compiled for the host. A clean device build proves it compiles and links for the target and
  nothing more; behaviour needs a flash, which Claude cannot do. See
  [working-with-claude.md](working-with-claude.md).
- **Once pip has a newer release, a device build writes to `~/.espressif`.** When the
  `esp-idf-sys` build script runs, it upgrades pip in ESP-IDF's Python environment there. The
  sandbox refuses that write, and the build ends in `Could not install esp-idf`. One device build
  outside the sandbox makes the upgrade, and builds here succeed again until pip's next release.
  `cargo build --release -p m5-core`, run in the device shell's directory, compiles `m5-core` and
  the crates it depends on for Xtensa without ESP-IDF, so it still works here.
- **`ps` and `pgrep` cannot list processes here**, so a run must write its own pid.
  `/usr/bin/time -l sh -c 'echo $$ > "$0"; exec "$@"' <pid file> <command>` does that and still
  reports the command's resource usage, since `exec` keeps the process.
- **`footprint` prints only `phys_footprint` and `phys_footprint_peak` here**, under an empty
  category table. Its text rounds to a whole GiB from 10 GiB up, so `/usr/bin/time -l` gives the
  exact peak. Point it at the pid with `footprint -p <pid> --sample 5`.
- **`/usr/bin/sample <pid> 10 -mayDie -file <path>` profiles a running process here**, with no code
  change or rebuild. Its call tree gives each function's share of each thread's samples.
- Say plainly, when reporting, which tests could not be run.
