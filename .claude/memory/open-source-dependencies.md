---
name: open-source-dependencies
description: prefer community-governed open-source libraries and formats, such as MapLibre's, over a vendor's, such as Mapbox's
metadata:
  type: feedback
---

Choose a community-governed open-source library or format over one a vendor controls, wherever one
exists. For maps, that means MapLibre's libraries and MapLibre Tile rather than Mapbox's libraries
and Mapbox Vector Tile. This holds even where the vendor's library is open source today:
`@mapbox/vector-tile` is BSD-licensed, and the preference still ruled it out.

**Why:** the user prefers not to depend on non-open-source libraries where possible. Mapbox
relicensed Mapbox GL JS as proprietary from v2, and MapLibre is the open fork of it.

**How to apply:** when weighing a dependency, name its licence and who governs it. Offer the
community-governed alternative first. Where none exists, or it costs a step, such as converting
tippecanoe's MVT output to MapLibre Tile, put the trade-off to the user.
