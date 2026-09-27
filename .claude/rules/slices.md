---
paths:
  - "**/docs/completed-slices.md"
  - "**/docs/current-slice.md"
---

# Slice Documentation Rules

- **completed-slices.md is append-only history**: never edit existing slice entries. Only add new entries when archiving a completed slice.
- **Checking off a task in current-slice.md**: just flip `[ ]` to `[x]`, leaving the task text as written. Don't append a summary of what you changed or where — the diff and commit history already record that. Add a note only when it conflicts with the task as written (e.g. you did it differently than described) or meaningfully enriches it (a decision or caveat a future reader needs).
- **Sections the work adds**: a slice starts with the sections
  [`decompose-slice`](../skills/decompose-slice/SKILL.md) gives it. Three more appear as the
  work runs, each only once there is something to put in it:
  - `Investigation` — what chasing a symptom established, and the evidence for it.
  - `Observations` — what a run or a field trip showed, dated where the date matters.
  - `Pending refactors` — a refactor the work revealed. It runs after the feature, so it goes here unticked rather than into the group in hand.
- **Task checkbox states**: `[ ]` = todo, `[x]` = done, `[-]` = moot/cancelled (no longer worth doing — e.g. superseded, or the premise was refuted). Use `[-]` rather than deleting or striking through the task, and add a short note saying why it's moot.
