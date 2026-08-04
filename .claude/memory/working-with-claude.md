# Working with Claude on this repo

Corrections and confirmed working practices, beyond what CLAUDE.md already states.

## Committing

Only run `git commit` after an explicit instruction to commit ("commit this", "please
commit"). Approval to *do* a task — "yes", "proceed", "next task" — is not approval to
commit its result: finish the work, propose a pithy commit message, and stop.

Even after an instruction to commit, expect the diff to be reviewed first. If a commit has
already been made and review was wanted, `git reset --soft HEAD~1` puts it back as staged
changes rather than discarding it.

Keep mechanical churn out of feature commits. When `cargo fmt` would touch files a change
never went near, stash the work, commit the formatting on its own, then restore and
continue.

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

## Reference docs

See [docs-style.md](docs-style.md).

## Tests

See [testing-limits.md](testing-limits.md) for what a passing run in the sandbox proves.
