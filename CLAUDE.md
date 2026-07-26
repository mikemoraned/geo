# geo

See [README.md](README.md) for what geo is. This file covers how to work on it.

geo is an umbrella monorepo for geo-related projects, containing different areas:
* `apps/`: individual apps, e.g. `linzer`
* `web`: a top-level website, which is a blog which lives at https://geo.houseofmoran.io
* `questions/`: data explorations, typically in notebooks, uv-managed
* `spikes/`: throwaway code used for experimentation and discovery that we keep around for reference of what was done, but code typically shouldn't be used directly
* `tools/`: shared tools e.g. a motis-server

## Memory (read before starting)

**Every memory belongs in this repo, in `.claude/memory/` — never in Claude Code's
external memory store.** That covers project knowledge, working preferences and
corrections alike: anything worth remembering is worth reviewing in a diff, sharing with
whoever else works here, and keeping in step with the code it describes. A note written
outside the repo is invisible to all of that, so if you find one there, move it in.

Read the relevant note before working on that area, and add or update one when you learn
something a future session shouldn't have to re-derive. Index new notes here. Current
notes:

- [`.claude/memory/lookout-architecture.md`](.claude/memory/lookout-architecture.md) —
  the lookout redis→sqlite pipeline as it stands, its migration to the medallion store,
  and the "Rust derives, Python reads" convention
- [`.claude/memory/motis-trips-api.md`](.claude/memory/motis-trips-api.md) — the Motis
  `map/trips` endpoint, enabling realtime, and the no-vehicle-GPS constraint
- [`.claude/memory/python-conventions.md`](.claude/memory/python-conventions.md) — keep
  Python minimal, in a uv project dir, no low-level fiddliness
- [`.claude/memory/docs-style.md`](.claude/memory/docs-style.md) — reference docs are
  written abstractly (no component names in rules) and in dry language
- [`.claude/memory/testing-limits.md`](.claude/memory/testing-limits.md) — Docker tests
  don't run in the sandbox, so binaries need running directly
- [`.claude/memory/working-with-claude.md`](.claude/memory/working-with-claude.md) —
  committing only on explicit instruction, and keeping formatting out of feature commits

## Running Claude

Launch Claude for an app under the safehouse sandbox with `just claude <app>`
(e.g. `just claude lookout`). This starts Claude with its working directory set to
`apps/<app>` — so the per-app slice skills resolve correctly — with read/write inside
that app and read-only across the rest of the repo (needed so Claude can discover the
shared `.claude/` skills and rules at the repo root).

## Methodology

Slices are tracked **per app**, in that app's own `docs/` dir (e.g.
`apps/lookout/docs/`). Run the slice commands from within the app you're working on —
they operate on the `docs/` relative to your current directory:

- `/choose-slice` — pick the next slice from `docs/next-slices.md` and promote it to
  `docs/current-slice.md`
- `/complete-slice` — archive the finished current slice into `docs/completed-slices.md`

Each app's slice docs:

- `docs/current-slice.md` — currently active slice and remaining tasks
- `docs/next-slices.md` — upcoming slices
- `docs/completed-slices.md` — append-only history of completed slices

The `/choose-slice` and `/complete-slice` skills and the `.claude/rules/slices.md` rule
are shared at the repo root and reused across apps.

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

<!-- geo-specific commands, invariants, and per-app notes go here as they emerge. -->

- **Code over comments:** make code self-documenting; add comments only for non-obvious
  things; substantive docs go in `docs/`.
- **Prefer existing libraries; don't hand-roll — especially in notebook cells.** Reach for a
  well-known library (numpy, scipy, scikit-learn, networkx, shapely/geopandas, matplotlib,
  lonboard's `colormap` helpers, …) instead of writing a bespoke algorithm: graph
  traversal / union-find, clustering, colour mapping, distance/geometry maths, hashing to
  values, etc. A cell should read as thin glue over library calls, not a mini-implementation.
  If nothing fits, say so and get agreement before hand-rolling.
