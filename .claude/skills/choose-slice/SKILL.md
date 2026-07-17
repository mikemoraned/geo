---
name: choose-slice
description: choose the next slice from next-slices.md and move it to current-slice.md
user-invocable: true
---

This skill is shared across apps. All `docs/…` paths below are **relative to the
current working directory**, so it operates on the slice docs of whichever app you're
running in (e.g. `apps/lookout/docs/…` when run from `apps/lookout`). Run it from an
app dir that has a `docs/` with the three slice files.

## Steps

1. **Read** `docs/current-slice.md`, and `docs/next-slices.md`.

2. **List remaining slices and ask user to choose next:**
   - In `docs/next-slices.md`, slices are identified by the `## Slice:` prefix.
   - Find all `## Slice:` headings in `docs/next-slices.md` ordered by occurrence in the doc
   - Provide a UI that allows the user to select which Slice they'd like to pick
   - Remember which they picked as "next slice"

3. **Promote the next slice to current-slice.md:**
   - Copy that slice's content **verbatim** (heading and all sub-content) into `docs/current-slice.md`, replacing its entire contents. Use the heading format: `# Current Slice:` followed by the tasks exactly as they appear.
   - **Remove** that slice (heading and all its content, up to the next slice heading or end of file) from `docs/next-slices.md`.

4. **Clean up:** If `docs/next-slices.md` is now empty (only the `# Next Slices` heading remains with no slice content), leave it with just the heading.

5. **Report** what you did: which Slice is now current
