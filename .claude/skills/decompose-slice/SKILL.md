---
name: decompose-slice
description: break the current slice into decisions and tasks before work on it starts
user-invocable: true
---

This skill covers breaking a slice into decisions and tasks, once it is current and before
any of it is built. [`slices.md`](../../rules/slices.md) covers the same doc during the work,
including the sections the work itself adds. Paths below are relative to the app dir you run
in.

## Steps

1. **Keep the Target and Straw Man as written.** They are the user's statement of intent,
   outline included.
2. **Read the code, docs, and store the slice touches.** The decisions name what already
   exists, and the tasks name where each change lands.
3. **Verify what the decisions depend on, or make verifying it the first task.** A size, a
   rate, or a response shape decides what gets built.
4. **Put each open decision to the user**, with its reason and its alternative. What they
   leave open goes under Open questions.
5. **Write every task, in every group, and leave no `...` behind.** Where a group's shape
   waits on what an earlier one finds, say so under Open questions.
6. **Propose a pithy commit message for what the pass settled, and stop.**

Expect several passes, each its own commit.

## The doc

`# Current Slice: <name>`, and the two sections every slice has:

- `### Target` — the user's. What exists at the end, deletions included.
- `### Tasks` — groups of checkboxes.

Nothing else is expected. Add one of the rest only where the slice has something to put in it:

- `### Straw Man` — the user's proposed approach, with any bias that settles later judgement
  calls.
- `### <the term>` — name it for the term the slice introduces or sharpens ("What a prediction
  is", "The entities", "The measure"). Say what the term excludes. A mermaid ER diagram where
  the terms are datasets.
- `### Decisions`, with the date the user confirmed them, under the line "Decisions taken
  before starting, as each changes what gets built". One bullet each: the claim in bold, the
  reason, the alternative it beats. The slice's detail lives here, not in its tasks. Past
  slices head this "Implementation Choices", "Decisions / groundwork", "Approach notes", or
  "Notes & Gotchas".
- `#### Consequences elsewhere` — what a decision forces on code the slice was not aiming at.
- `#### Rejected / deferred` — an option ruled out, and what would bring it back. Writing it
  down stops it coming up again.
- `#### Open questions` — what stays unsettled, and what the answer changes.

## Groups

Two groups have names of their own. `#### Refactors / extensions` comes first where the slice
needs existing code changed before the new work fits. `#### Wrap-up` comes last: the recipes
to capture, and the checks to run.

Otherwise group by deliverable — a crate, a dataset, a page — and head each group with what it
delivers. Where integration risk dominates, group by phase instead, and give each phase its
reason. A first phase carries a throwaway core the whole way, so new plumbing fails before
anything real sits behind it.

Order so the code keeps working. De-risk the largest unknown first. Measure before a later
decision needs the number. Rename before the moves that would land under the old name. Delete
last, once the facts worth keeping sit elsewhere.

## Tasks

The list stages the work in order. Keep each task to a line: an imperative, and the crate,
file, type, column, flag, or recipe it touches. Naming those keeps a task concrete, as slice
docs are — see [`docs-style.md`](../../memory/docs-style.md). The reason, the alternative, the
cases to cover, and the evidence go under Decisions. A task grown to a paragraph is a decision
in the wrong place.

Two things a task carries beyond the step itself:

- **What it waits on** — "only after X lands". Opportunistic work says so: "low-priority, do
  while already in the crate".
- **What it leaves to a later task**, in a few words, where the boundary is easy to cross.

A task can be a decision or a question rather than a change. A threshold comes from data an
earlier task produced, not from the decomposition. An invariant in the Target becomes a task
that measures it. A dated note the slice grew from gets a closing task folding it into the
code and deleting it.

Don't restate the Target as tasks, and don't write a catch-all: "update the docs where
needed" names neither the doc nor the change.
