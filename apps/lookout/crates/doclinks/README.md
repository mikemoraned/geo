# doclinks

Checks that the links between this app's docs resolve: every relative target exists, and every
`#heading` names a heading the target actually has. The crate is nothing but that check — no binary,
and nothing depends on it — so `just test` and `just test-no-docker` run it with everything else.

Its scope is the app: every `.md` under `apps/lookout`, skipping dotted directories and the
generated trees (`target`, `.venv`, `node_modules`, `site-packages`). The docs above the app —
`CLAUDE.md`, the rules — are out of its reach, and so unchecked.

`pulldown-cmark` does the reading, so a link inside a code span or a fenced block is not a link, and
a `#` inside one is not a heading. What is left here is the anchor convention: a heading is compared
as GitHub slugs it, lowercased with punctuation dropped and spaces turned to hyphens, which is the
form a link between these docs has to use. A URL is left alone, and a `#heading` with no path is
checked against the page it is written on.
