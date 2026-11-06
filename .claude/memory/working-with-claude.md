# Working with Claude on this repo

Corrections and confirmed working practices, beyond what CLAUDE.md already states.

## Committing

Only run `git commit` after an explicit instruction to commit ("commit this", "please
commit"). Approval to *do* a task — "yes", "proceed", "next task" — is not approval to
commit its result: finish the work, propose a pithy commit message, and stop.

Even after an instruction to commit, expect the diff to be reviewed first. An instruction to
commit covers only a diff the user has seen. When code went in after the last diff shown, or
none was shown, show what the commit holds and wait. If a commit has already been made and
review was wanted, `git reset --soft HEAD~1` puts it back as staged changes rather than
discarding it.

Keep mechanical churn out of feature commits. When `cargo fmt` would touch files a change
never went near, stash the work, commit the formatting on its own, then restore and
continue.

Work that runs ahead of approval can still land as one commit per step. After each step,
record the working tree as a tree object through a scratch index. That commits nothing and
leaves the real index alone:

```sh
GIT_INDEX_FILE=<scratch>/step.index git read-tree HEAD
GIT_INDEX_FILE=<scratch>/step.index git add <the paths the work touched>
GIT_INDEX_FILE=<scratch>/step.index git write-tree
```

The last command prints the tree's hash. Once the user approves the commits, `git read-tree
<hash>` loads a step into the real index for `git commit`. Each tree holds every step before
it, so commit the steps in order. If `HEAD` moves first, as when another session commits
polls, rebuild each tree on the new `HEAD`.

## Decomposing a slice

A decomposition plans a slice, and leaves the solving to the slice's tasks, so the user can judge
each step as it lands. On 2026-11-03, a re-run of `/decompose-slice` measured the store, ran a
throwaway preview of several predictors, and rebuilt the slice around the results. The user had no
way to assess any of it, and reverted it.

- Keep the plan small enough to read and check, and build it up from scratch in steps.
- Ask the user rather than measure. Where a decision depends on a number, make measuring it a task.
- Change only the current slice. Another slice's text stays as written unless the user asks.
- Write in plain words. "The chosen settings" says what "operating point" says, without the jargon.

## Memos and caches

A memo that saves much time shows that something upstream repeats work. Find why the same input
comes back, and remove the repeat there. Keep a memo only where no upstream change removes the
repeat. On 1 November 2026, a memo of country lookups saved 156 s, and its repeats came from a read
that joined each polyline's text onto every leg.

## Flashing the M5 device

Claude cannot open the serial port: every `/dev/cu.*` open returns `EPERM` from its process
tree, including with the Bash sandbox disabled, so it is a session-level restriction with no
workaround. Build and verify in-session, then hand over `! cd <spike-dir> && just flash` and
read the boot log pasted back. Plan device work around that: put anything checkable into the
startup log, so one flash round-trip answers the question.

## Spikes

Throwaway code written to answer a question — does this library accept this shape, does
this round-trip — is **committed and then removed in a later commit**, not deleted before
committing. The question, the code that answered it and the answer then sit in the
history, where the next person to ask the same question can find them. A spike deleted
before it is committed leaves only its conclusion, with no way to check the conclusion.

Say in the spike commit message what is being asked; say in the removing commit what the
answer was.

## Migrating the store

A one-off migration of the store, such as a backfill binary, follows the spike rule: commit it
with the data it rewrote, then remove it in a later commit.

Write the migration at the level of the row types. Map each old row type to the new one in Rust.
Read the old version through medallion's `Query`, and write the new version through its writers.
Reading parquet files directly, or parsing the store's file and partition names, repeats layout
rules that medallion holds. Copy only what those interfaces show. A file's write instant does
not show through them, so the copy's files carry the instant of the copy.

Pause before deleting the old version's files or the migration binary, so the user can inspect
what the migration did.

## Notebooks

See [`.claude/rules/lookout-notebooks.md`](../rules/lookout-notebooks.md).

## External services

Claude sends no request to an external service such as Transitous, not even one within its usage
policy. The user runs every capture or check against one. When a change needs measuring against a
live service, apply it to the real code in small steps, and hand over the command for each.

## Reference docs

See [docs-style.md](docs-style.md).

## Tests

See [testing-limits.md](testing-limits.md) for what a passing run in the sandbox proves.

## Shell

Keep shell plain: a recipe runs commands in sequence, with at most a simple `if`. When a recipe
starts to need arrays, parameter-expansion tricks, or workarounds for a shell's version, move
the logic into Rust or Python. macOS ships bash 3.2, so a construct that works in a newer bash
can fail there. One example: under `set -u`, bash 3.2 rejects an empty array as unbound.

## Builds and disk

A test build of the whole workspace in the dev profile fills tens of gigabytes. With full debug
info it took `target/debug` to 43 GB. With `CARGO_PROFILE_DEV_DEBUG=line-tables-only` it took
17 GB, 16 GB of it the workspace's own test binaries, each of which links DataFusion. During those
builds swap reached 17 GB, and swap files take space on the same disk.

When a build would relink the workspace:

1. Ask the user whether a build or test run of theirs is using the target directory. `cargo clean`
   does not wait for a running build, and the sandbox shows no other process.
2. Clear the workspace crates' dev artefacts. `cargo clean --profile dev`, with a `-p` for each
   package `cargo metadata --no-deps` lists, keeps the third-party builds.
3. If less than 25 GB is free, build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`,
   `CARGO_INCREMENTAL=0`, and two build jobs. The first build after the switch recompiles every
   third-party crate.
4. Watch free disk until the build ends. The capture's poller writes to the same disk, and a full
   disk loses its polls.

Clearing can free far less than it removes, because a Time Machine local snapshot keeps every file
that existed when it was taken. On 2 November 2026, about 195 GiB deleted after one snapshot freed
under 3 GiB. Free disk rose from 11 GiB to 185 GiB once `tmutil deletelocalsnapshots <date>` ran.
The command printed a failure all the same, so judge it by free disk. The sandbox hides all this:
`tmutil listlocalsnapshots /` lists nothing there, and `diskutil` cannot reach its daemon, so the
user runs both `tmutil` commands. Check free disk after clearing, and keep the third-party builds
unless the space is needed.

A cargo command on a python extension's crate slows the next uv build of that extension. On
6 November 2026, uv rebuilt `evaluation-py` in 9 minutes after `cargo clippy` ran on its crate, and
in 36 s with nothing run between. Run cargo on an extension's crate after its python tests, not
between two of their builds.

## Cargo

`cargo remove` also deletes every `[workspace.dependencies]` entry that no member of the
workspace uses, and the lookout workspace keeps some of those on purpose. After running it, check
`git diff Cargo.toml` at the workspace root, and restore any entry it pruned.

## Rejected designs

When a review rejects a design, correct or remove every uncommitted doc and comment that still
states it. Do it before reporting the review, not in a later rewrite. Prose reads as current: a
README left for a later rewrite once described a credential-ID leak the review had already
rejected.
