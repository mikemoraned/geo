# Current Slice: milestones above slices

### Target

`next-slices.md` is a flat list of slices, and its order is the only sign of what comes next or
why. A milestone groups slices under one intent. At the end, `next-slices.md` holds milestones,
each with a Target stating what its slices achieve together. Each slice sits under one milestone,
or under none.

Moving a slice into a milestone, out of one, or between two moves its block in the doc. Nothing
inside the block changes. The slices rule and the slice skills describe milestones, and the user
has placed every slice now in `next-slices.md`.

### Tasks

#### The format

- [ ] Decide the headings for a milestone and its Target. A slice keeps its heading levels under
      any milestone. The candidate is a `# Milestone: <name>` heading above the `## Slice:` blocks.
- [ ] Decide where a slice with no milestone sits: under an `# Unassigned` heading, or above the
      first milestone.
- [ ] Decide whether `current-slice.md` and `completed-slices.md` name a slice's milestone.

#### The rule and the skills

- [ ] Describe milestones in `.claude/rules/slices.md`: the headings, the Target, and moving a
      slice by moving its block.
- [ ] In `.claude/skills/choose-slice/SKILL.md`, list the slices grouped by milestone. End a
      slice's block at the next slice heading or milestone heading.
- [ ] Carry the milestone through `choose-slice` and `complete-slice`, if the format decision
      names it in `current-slice.md`.
- [ ] State in the Methodology section of `CLAUDE.md` that `next-slices.md` groups slices by
      milestone.

#### The milestones

- [ ] Restructure `docs/next-slices.md` into the new format, with every slice unassigned.
- [ ] With the user, name each milestone and write its Target.
- [ ] With the user, place each slice in `docs/next-slices.md` under a milestone, or leave it
      unassigned.

#### Wrap-up

- [ ] Run `/choose-slice` against the new `docs/next-slices.md`, and stop before it promotes a
      slice. Its list groups the slices by milestone.
