# prose-gate

A Claude Code Stop hook. A session that changed prose has to have invoked the skills the prose
rules live in, and this reports when one of them was never called.

## What it does

The hook reads its payload on stdin and prints a blocking decision when both of these hold:

- a file matching `--glob` has changed in the repository, staged, unstaged or untracked; and
- a skill named by `--require` appears nowhere in the session transcript.

The decision names the skills that are missing and the files that changed. Claude then invokes
what it missed and takes the prose through those rules, or says the change carries no prose.

Anything the gate cannot establish leaves the session alone: a payload it cannot parse, a
directory outside a repository, an unreadable transcript, a git that fails. A gate that blocks on
its own confusion costs more than the rule it guards.

## Reading a transcript

A transcript is JSON Lines. The gate counts a skill as invoked where an object carries
`"type": "tool_use"`, `"name": "Skill"`, and a string under `input.skill` naming it. Each of the
three matters:

- The transcript holds the *definition* of the Skill tool as well as every call of it, and the
  definition names the tool too. It declares `skill` as a schema rather than a string, so reading
  the string tells a call from a declaration.
- Prose that names a skill reaches the transcript whenever a session reads the file it sits in —
  including the rule this gate enforces. Quoting a rule therefore satisfies nothing.

The search walks the whole of each line rather than a fixed path into it, so nesting can move.
Unparseable lines are skipped, since a transcript is written as the session runs and its last
line can be half-written.

## Running it

    just build   # the release binary the hook names
    just test    # the tests

From the repository root, `just prerequisites` builds it alongside the other repo-wide tools.

The hook is wired in `.claude/settings.json`, which names the binary and the skills to require.
Build it once per checkout.

## Limits

The transcript is written asynchronously, so a skill invoked moments before the hook fires can be
absent from the file. What bounds the cost of that is `stop_hook_active`, which the payload sets
once a Stop hook has blocked the turn already: the gate stands down when it is true, so a session
is blocked at most once and the stop that follows sees the flushed line. Reading it the other way
round — checking only when a block has already fired — blocks nothing ever, since the gate itself
is what sets the flag.
