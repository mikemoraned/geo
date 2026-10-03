# Current Slice: milestones above slices

### Target

`next-slices.md` is a flat list of slices, and its order is the only sign of what comes next or
why. A milestone groups slices under one intent. At the end, `next-slices.md` holds milestones,
each with a Target stating what its slices achieve together. Each slice sits under one milestone,
or under none.

Moving a slice into a milestone, out of one, or between two moves its block in the doc. Nothing
inside the block changes. The slices rule and the slice skills describe milestones, and the user
has placed every slice now in `next-slices.md`.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-10-03.

- **A milestone is a `# Milestone: <name>` heading with a `## Target` section, above its
  `## Slice:` blocks.** Level-one headings leave every slice at level two, so a slice moves
  between milestones with no change inside its block. The Target comes before the milestone's
  first slice, so a slice's block still ends at the next `## Slice:` or level-one heading.
  Milestones at level two would tidy the outline, but every move would re-level a slice's headings.
- **A slice with no milestone sits under `# Unassigned`, the last heading in the doc.** Planned
  work comes first, and a slice leaves its milestone by moving below that heading. Slices above
  the first milestone, with no heading of their own, carry nothing that marks them unassigned.
- **Only `next-slices.md` names milestones.** `current-slice.md` and `completed-slices.md` carry
  no milestone line, and a slice returned from `current-slice.md` is placed by hand. A
  `Milestone:` line written by `/choose-slice` would tell `/complete-slice` which milestone the
  slice came from, at the cost of a line in every slice doc.
- **`/complete-slice` archives a milestone with no slices left, once the user confirms it is
  done.** It appends the milestone's name and Target to `completed-slices.md` as a
  `## Milestone:` entry, and removes the milestone from `next-slices.md`. `/choose-slice` never
  removes a milestone, even when it takes the last slice. Deleting a finished milestone outright
  leaves no record of its intent.

#### Consequences elsewhere

- With no milestone line, `/complete-slice` cannot tell which milestone the finished slice came
  from. It asks about every milestone with no slices left instead.
- A milestone's archive entry holds its Target, not its slices, since the slice entries do not
  name it.
- `/choose-slice` step 4 treats a doc holding only `# Next Slices` as empty. Under milestones, a
  doc with no slices still holds `# Unassigned` and any milestone awaiting archive.
- The Methodology section of `CLAUDE.md` describes `next-slices.md` as "upcoming slices".

### Tasks

#### The rule and the skills

- [ ] Describe milestones in `.claude/rules/slices.md`: the `# Milestone:` and `## Target`
      headings, `# Unassigned` last, and moving a slice by moving its block.
- [ ] In `.claude/skills/choose-slice/SKILL.md`, list the slices grouped by milestone, with the
      unassigned ones last. End a slice's block at the next `## Slice:` or level-one heading.
- [ ] In the same skill, rewrite step 4: a doc with no slices keeps `# Unassigned` and its
      milestones.
- [ ] In `.claude/skills/complete-slice/SKILL.md`, add a step after the archive: for each
      milestone with no slices left, ask whether it is done. Archive each done one as a
      `## Milestone:` entry and remove it from `docs/next-slices.md`.
- [ ] Describe `next-slices.md` in the Methodology section of `CLAUDE.md` as upcoming slices,
      grouped by milestone.

#### The milestones

- [ ] Restructure `docs/next-slices.md` into the new format, with every slice under
      `# Unassigned`.
- [ ] With the user, name each milestone and write its Target.
- [ ] With the user, place each slice in `docs/next-slices.md` under a milestone, or leave it
      unassigned.

#### Wrap-up

- [ ] Run `/choose-slice` against the new `docs/next-slices.md`, and stop before it promotes a
      slice. Its list groups the slices by milestone.
