---
paths:
  - "**/*.rs"
  - "**/*.py"
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

## Applying it to code that has comments

Deleting the comment is one answer; absorbing it is the better one. Rename what it explained,
split the function it summarised, or give the value it described a type. Behaviour stays as it
was, and the tests stay green.
