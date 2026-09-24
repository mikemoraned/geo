---
paths:
  - "**/*.rs"
  - "**/*.py"
  - "**/*.js"
---

# The Does-It-Bring-Joy Rule

**Write no comment by default** — not `//`, `///`, `//!`, nor a docstring. Say it in the naming
and the structure instead, and name a well-known pattern wherever one fits.

A comment survives only where you can justify that one: what it says, no name, no type and no
smaller function can carry, and a reader loses something real without it. A doc comment on a
public item is no exception, and faces the same question.

Whatever survives is prose, so it goes through `writing-clearly-and-concisely` and
`technical-writing@technical-writing`.

## Where the rest goes

- **A convention, used or invented** — the crate's README or another `.md`, away from the code.
- **A durable fact about the system** — the hardware, an upstream API, a data format, the shape
  of the pipeline — the app's `docs/`, as `CLAUDE.md` directs.
- **Why a change was made** — the commit message and the slice doc.
- **Everything else** — a better name.

## Text a tool reads is not a comment

A CLI argument's help, a doctest, a macro's input: the compiler or the program consumes it, and
removing it changes what the program does. Write what is needed, and hold it to the same prose
standards as anything else a human reads.

## The rule stays out of the prose

No README, `docs/` page or other `.md` explains this rule, lists which comments survived a
sweep, or defends one. Docs describe the system; a note about what the code does or does not
say about itself describes the code's housekeeping, and belongs nowhere a reader of the system
has to walk past it. Where a survivor needs a case made, make it in the commit message.

## Applying it to code that has comments

Deleting the comment is one answer; absorbing it is the better one. Rename what it explained,
split the function it summarised, or give the value it described a type. Behaviour stays as it
was, and the tests stay green.

A file this repo did not write is out of it: a vendored library or a generated binding is read
rather than maintained, and editing one only makes the next upgrade a merge.
