# geo

See [README.md](README.md) for what geo is. This file covers how to work on it.

geo is an umbrella monorepo for geo-related projects, containing different areas:
* `apps/`: individual apps, e.g. `linzer`
* `web`: a top-level blog, at https://geo.houseofmoran.io
* `questions/`: data explorations, typically in notebooks, uv-managed
* `spikes/`: throwaway code from experiments, kept as a record of what was done; don't use it directly
* `tools/`: shared tools e.g. a motis-server

## Memory (read before starting)

**Every memory belongs in this repo, in `.claude/memory/` — never in Claude Code's
external memory store.** That covers project knowledge, working preferences, and
corrections alike: anything worth remembering is worth reviewing in a diff, sharing with
whoever else works here, and keeping in step with the code it describes. A note written
outside the repo is invisible to all of that, so if you find one there, move it in.

Read the relevant note before working on that area, and add or update one when you learn
something a future session shouldn't have to re-derive. Index new notes here.

**Notes hold how we work; `docs/` holds what we have discovered.** A fact about the system
itself — the hardware, an upstream API, a data format, the shape of the pipeline — belongs in
an app's `docs/`, written for anyone working on the code rather than for Claude. Notes are
for working practice and for the limits of this environment. Current notes:

- [`.claude/memory/python-conventions.md`](.claude/memory/python-conventions.md) — keep
  Python minimal, in a uv project dir, no low-level fiddliness
- [`.claude/memory/docs-style.md`](.claude/memory/docs-style.md) — reference docs are
  written abstractly (no component names in rules) and in dry language
- [`.claude/memory/testing-limits.md`](.claude/memory/testing-limits.md) — Docker tests
  don't run in the sandbox, so binaries need running directly
- [`.claude/memory/working-with-claude.md`](.claude/memory/working-with-claude.md) —
  committing only on explicit instruction, keeping formatting out of feature commits, and
  why Claude cannot flash the device

Lookout's own facts are in [`apps/lookout/docs/`](apps/lookout/docs/): `medallion.md` (the
store), `architecture.md` (the pipeline), `telemetry.md` (the wire a device sends),
`device.md` (the M5 board and its GNSS receiver), `motis.md` (the Motis API and the German
timetable feed), `overture.md` (the reference source and what an extract takes).

## Running Claude

Launch Claude from the repo root with `just claude`, which runs it under the safehouse sandbox.
The sandbox grants read/write inside the repo. `~/.espressif` is granted read-only, so a device
build uses the one shared ESP-IDF install rather than several gigabytes under every worktree. Only
the first install writes there, and that one runs outside the sandbox.

## Prose: invoke the two writing skills before drafting

Any turn that will write or edit prose a human reads — `docs/`, READMEs, slice docs, commit
messages, comments, error messages — starts by invoking both skills with the Skill tool:

- `softaworks-agent-toolkit-writing-clearly-and-concisely`
- `technical-writing:technical-writing`

Invoke them before the first draft, not after. Drafting from memory of the rules and then
reviewing against them fails: the sentences are already written, and the review reads as
proofreading rather than rewriting, so the reader ends up copyediting word by word. Applying the
rules without invoking the skills does not count — if neither appears in the turn, the prose task
is unstarted, however finished the text looks.

A Stop hook enforces this. When a session changed prose without invoking both skills, the hook
blocks the stop and names the missing one. [`tools/prose-gate`](tools/prose-gate/README.md) is the
gate, `.claude/settings.json` wires it, and `just prerequisites` builds the binary it names — once
per checkout. It reads the transcript for the invocation rather than for the skill's name, so
quoting a rule satisfies nothing.

[Conventions / Style](#conventions--style) states what each skill covers and the three
departures this repo takes from them.

## Methodology

Slices are tracked **per app**, in that app's own `docs/` dir (e.g.
`apps/lookout/docs/`). Run the slice commands from within the app you're working on —
they operate on the `docs/` relative to your current directory:

- `/choose-slice` — pick the next slice from `docs/next-slices.md` and promote it to
  `docs/current-slice.md`
- `/decompose-slice` — break the current slice into decisions and tasks, before building any
  of it
- `/complete-slice` — archive the finished current slice into `docs/completed-slices.md`

Each app's slice docs:

- `docs/current-slice.md` — currently active slice and remaining tasks
- `docs/next-slices.md` — upcoming slices, grouped by milestone
- `docs/completed-slices.md` — append-only history of completed slices

The three slice skills and the `.claude/rules/slices.md` rule are shared at the repo root
and reused across apps.

### Test-Driven Development

When doing TDD, always keep the code compiling at every step:
1. Write a stub that compiles but returns a wrong/trivial value (e.g. `0`, `false`, `""`)
2. Write tests asserting the correct behaviour — they should **fail** (wrong value, not compile error)
3. Implement correctly — tests should now pass

## Committing

- **Never `git commit` (or `git push`) without explicit approval.** Make the changes, then stop.
- At a natural commit point (or when asked to stop), propose a **pithy** commit message —
  minimal, capturing intent plus any significant changes — and show it for review. Do not commit yet.
- Commit only after the user says to. "Give me a commit message" / "what's the commit" means
  **show** the message, not run the commit.

## Conventions / Style

- **Write all prose through the two writing skills**, invoked as
  [Prose](#prose-invoke-the-two-writing-skills-before-drafting) requires. They apply to edits of
  existing prose as much as to new prose: documentation, READMEs, commit messages, error
  messages, comments, and anything else a human reads.
  - `softaworks-agent-toolkit-writing-clearly-and-concisely` carries the general principles —
    active voice, positive form, concrete language, no needless words, and none of the puffery
    it lists.
  - `technical-writing:technical-writing` carries what technical text additionally needs — one
    term per concept, sentence-length caps, condition before command, procedural and
    descriptive passages kept apart, and no hedging (`should`, `may`, `simply`, `just`).
  - Reference docs additionally follow
    [`.claude/memory/docs-style.md`](.claude/memory/docs-style.md).
  - Three deliberate departures. **British spelling** throughout (`visualise`, `colour`,
    `metres`) and **`data` as a singular mass noun** ("data is stored") override the 1918
    source. **Permission-sense `may`** ("a projected column may additionally be
    pre-computed") stays, against the hedging rule, since it states what the rules allow
    rather than softening a claim; possibility-sense `may` still becomes `can`. Everything
    else in both skills applies, including the serial comma.
- **Write no comment by default** — see
  [`.claude/rules/does-it-bring-joy.md`](.claude/rules/does-it-bring-joy.md), which covers Rust,
  Python, JavaScript and `Justfile`s. Naming and structure carry what a comment would have said; a convention goes in a
  README, and a durable fact about the system goes in the app's `docs/`. A comment survives
  only where you can justify that one.
- **Prefer existing libraries; don't hand-roll — especially in notebook cells.** Reach for a
  well-known library (numpy, scipy, scikit-learn, networkx, shapely/geopandas, matplotlib,
  lonboard's `colormap` helpers, …) instead of writing a bespoke algorithm: graph
  traversal / union-find, clustering, colour mapping, distance/geometry maths, hashing to
  values, etc. A cell should read as thin glue over library calls, not a mini-implementation.
  If nothing fits, say so and get agreement before hand-rolling.
