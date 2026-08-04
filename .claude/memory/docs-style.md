# Reference docs style

How docs under `docs/` (as opposed to slice docs) are written in this repo.

- **Describe patterns abstractly; don't anchor rules to concrete components.** Naming
  specific crates, binaries, tables or services inside a rule ("redis, the
  `lookout-telemetry` list, is landing"; "`motis_poll` writes here") dates the doc as soon
  as those parts move. State the rule over the role instead: "the capture log a polling
  process appends to". Concrete examples are occasionally worth including, but they decay
  and need maintaining, so the abstract description carries the weight.
- **Use dry, impassionate language.** No colloquialism or emphasis for its own sake:
  "not allowed" rather than "not on the table"; "permitted" rather than "fine and
  expected"; "expected at this layer" rather than "the point here". Prefer plain
  declaratives ("a projected column may additionally be pre-computed") over exhortation
  ("**Always** pre-compute…").
- **State what holds now, not how it came to be known.** A reference doc is not a record of
  the work that produced it, so remove the retellings: which diagnoses were wrong before the
  right one, what an earlier estimate assumed, what a superseded workaround did. A fact that
  still constrains a decision stays — a measurement and its sample size, a reproducible
  symptom and the versions it appears in, a suspect ruled out with the evidence against it —
  but it is stated as a standing property rather than narrated as an episode. Write
  comparisons with an alternative in the present tense, since past tense turns a live
  rationale into an anecdote. The history itself belongs in the slice record.

This applies to long-lived reference docs. Slice docs (`current-slice.md`,
`next-slices.md`, `completed-slices.md`) are a record of specific work and stay concrete.
