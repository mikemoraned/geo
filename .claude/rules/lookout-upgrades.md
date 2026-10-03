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
