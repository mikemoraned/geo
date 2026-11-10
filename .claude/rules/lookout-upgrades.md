---
paths:
  - "apps/lookout/**"
---

# Lookout Upgrades

`just outdated`, run from `apps/lookout`, lists each pinned version that has a newer release. It
covers the crates of both cargo workspaces, the direct dependencies of each uv project, the
SedonaDB tag, and the ESP toolchain. A run takes about 20 seconds, most of it cargo reading the
crates.io index.

Run it when a slice is chosen. If the slice touches something the report lists, upgrade that first,
in a commit of its own. Everything else waits for an upgrades slice, chosen like any other.

`just audit`, run from `apps/lookout`, checks every crate in `Cargo.lock` against the RustSec
advisory database. Run it when a slice is chosen, beside `just outdated`. If it flags a crate, run
`just audit-update`, and commit the changed `Cargo.lock` on its own. That recipe moves each flagged
crate to its newest compatible release, whether or not the slice touches it, and then audits again.
A crate the second audit still flags needs a major version or a dependent's upgrade, so it waits
for an upgrades slice.
