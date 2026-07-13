# geo

See [README.md](README.md) for what geo is. This file covers how to work on it.

geo is an umbrella monorepo for geo projects: `apps/` (individual apps, e.g. `keks`),
a shared `backend` cargo workspace, a top-level `web` site, `questions/` (Python data
explorations, uv-managed), `spikes/`, and `tools/`.

## Methodology

We follow a Walking Skeleton approach: incremental, thin, end-to-end slices — each one
something you could ship and observe working, rather than a horizontal layer.

Slices are tracked in `docs/` and driven by two commands:

- `/choose-slice` — pick the next slice from `docs/next-slices.md` and promote it to
  `docs/current-slice.md`
- `/complete-slice` — archive the finished current slice into `docs/completed-slices.md`

Slice docs:

- `@docs/current-slice.md` — currently active slice and remaining tasks
- `@docs/next-slices.md` — upcoming slices
- `@docs/completed-slices.md` — append-only history of completed slices

See `.claude/rules/slices.md` for how to edit the slice docs.

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
