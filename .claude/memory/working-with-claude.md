# Working with Claude on this repo

Corrections and confirmed working practices, beyond what CLAUDE.md already states.

## Committing

Only run `git commit` after an explicit instruction to commit ("commit this", "please
commit"). Approval to *do* a task — "yes", "proceed", "next task" — is not approval to
commit its result: finish the work, propose a pithy commit message, and stop.

Keep mechanical churn out of feature commits. When `cargo fmt` would touch files a change
never went near, stash the work, commit the formatting on its own, then restore and
continue.

## Reference docs

See [docs-style.md](docs-style.md).

## Tests

See [testing-limits.md](testing-limits.md) for what a passing run in the sandbox proves.
