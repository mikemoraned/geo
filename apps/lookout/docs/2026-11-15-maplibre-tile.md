# MapLibre Tile against Mapbox Vector Tile

*Assessment of 2026-11-15. It built no tile and ran no tool. The evidence is the
[MapLibre Tile specification](https://maplibre.org/maplibre-tile-spec/), its
[repository](https://github.com/maplibre/maplibre-tile-spec), the
[PMTiles v3 specification](https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md), and the
[MapLibre style specification](https://maplibre.org/maplibre-style-spec/sources/). Whatever the slice
adopts belongs in [web.md](web.md) and [medallion.md](medallion.md), and the rest goes stale.*

Whether the map artefact's tiles are better encoded as MapLibre Tile (MLT) than as Mapbox Vector Tile
(MVT). It is for whoever works on the slice "Spike on Maps using Kiosk display as an example", and
for the later slice that shows a map in the App.

## What MLT is

MLT is a vector tile format that stores each geometry and each attribute as its own column. Each
column takes an encoding chosen for its data type, such as delta, run-length, or dictionary
encoding. MVT stores a tile as a sequence of features, each with drawing commands and key-value
tags.

The format has two versions:

- **v1** is the stable wire format. The repository README calls the specification stable as of
  October 2025. v1 holds 2D features with scalar attributes, the same data model as MVT. It is
  stricter than MVT: an attribute has one type across a layer, and a missing attribute becomes a
  typed null.
- **v2** is experimental, and its layout can change between releases without a version bump. It
  adds an elevation per vertex, any number of measure values (M-values) per vertex, and lists and
  maps as attributes. MapLibre's renderers cannot read v2 yet.

Both versions keep MVT's integer tile grid. A coordinate is a signed integer in a grid of `extent`
units per tile side, and the default extent is 4096. v1 sets no upper bound on the extent. v2
allows powers of two from 64 to 2,097,152.

The specification's headline figures compare planet-wide archives. Uncompressed MLT v2 is 4–8%
smaller than gzipped MVT and decodes 2.3–3.7 times faster. With gzip, v2 is 10–18% smaller.

## Support

| | MVT | MLT |
| --- | --- | --- |
| PMTiles tile type | `0x01` | `0x06`, "MapLibre Vector Tile" |
| MapLibre GL JS | Every version | 5.12.0 and later, with `"encoding": "mlt"` on the source |
| Built by tippecanoe | Yes | No. Support is listed as ongoing work |
| Built by Planetiler | Yes | 0.10.0 and later |
| Converted from MVT | — | The `mlt convert` CLI, from an MVT `.pmtiles` or `.mbtiles` archive. It writes v1 by default |
| Decoded by a page outside the renderer | `@mapbox/vector-tile` | `@maplibre/mlt` 1.3.0, `decodeTile(bytes)` |
| Decoded in Rust | Several crates | `mlt-core`, which Martin uses |

## What the comparison leaves unverified

- Whether `@maplibre/mlt` ships a build that a page without a bundler can import. Its
  `package.json` names a `tsc` build with no `module` or `exports` field.
- What a decoded feature looks like, and so how a page reads a sample's attributes from one.
- Whether `mlt-core` and the `mlt` CLI are published on crates.io. The README installs the CLI
  from the repository.
- Whether `mlt-core` builds without the standard library, which the device would need.
- Whether the `pmtiles` JavaScript library passes a `0x06` tile to MapLibre unchanged. Its README
  says nothing on tile types.
- The first iOS release of MapLibre Native to read MLT. The specification site says 6.2.0, and the
  style specification says 6.20.0.

## How MLT bears on the slice

**MLT changes no decision the slice has taken.** v1 still quantises a coordinate to the tile grid,
so a sample still carries its full values as attributes. A larger extent would hold a position
more precisely, but attributes need no such tuning. The `samples` layer, the corridor, and the
zoom levels are the same in either encoding.

**MLT changes the build and the reader.** tippecanoe writes only MVT, so an MLT artefact takes a
second step: `mlt convert` over the MVT archive tippecanoe wrote. The map source gains
`"encoding": "mlt"`. The replay swaps `@mapbox/vector-tile` and `pbf` for `@maplibre/mlt`, whose
use from a page without a bundler is unverified.

**The gains are small at this artefact's size.** The artefact holds a few routes and the rail and
water within 5 km of them. A 10–18% saving on a file of that size is unlikely to matter to the
kiosk. Decode speed matters more on the device than in the browser, and the device has no map yet.

**v2's M-values fit the replay better than either format does today.** A route stored as one line,
with each vertex's time as an M-value, holds positions and timings in one feature. That replaces a
layer of points with attributes. v2 is experimental, and no renderer reads it, so this waits.

## What would make MLT worth adopting

- The App shows a map on the device, and a Rust MLT decoder builds for it. Decode speed and size
  then count against flash and CPU, not against a browser.
- tippecanoe writes MLT, which removes the conversion step.
- MLT v2 becomes stable and MapLibre reads it. The route and its timings then fit in one feature.

## Outcome

The slice adopts MLT v1, on a preference for MapLibre's libraries and formats over Mapbox's,
rather than for size or speed. tippecanoe still tiles, and `mlt convert` encodes its MVT output as
MLT. The MVT file is an intermediate, and no page reads it. The replay decodes with
`@maplibre/mlt`, so whether a page without a bundler can import it is the first thing the slice
checks.
