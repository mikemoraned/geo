# map

Packs the map a kiosk draws and replays into one [PMTiles](https://docs.protomaps.com/pmtiles/)
file: the sessions worth replaying, and the rail and water that the water crossings name.

```sh
just gold-pack-map                                         # defaults, run from apps/lookout
just gold-pack-map --medallion-root <store> --output <dir>
just kiosk-map                                             # copy the adopted version to static
```

`pack_map` chooses the sessions as the session crossings' gold derivation does, by
`--min-crossings` and `--max-sessions`. `gold-pack-map` adopts what it packs in `map.version`.

## The layers

| Layer | Geometry | Zoom levels | Attributes |
| --- | --- | --- | --- |
| `route` | one line per session | 0–12 | `session`, `crossings`, `samples_zoom`, `min_lon`, `min_lat`, `max_lon`, `max_lat` |
| `samples` | one point per sample | 6 only | `session`, `seq`, `t`, `lat`, `lon`, `alt`, `acc`, `speed`, `heading` |
| `rail` | each rail segment a crossing names | 7–12 | `class` |
| `water` | each water feature a crossing names | 7–12 | `subtype`, `class` |

A reader lists the sessions from `route` at the file's lowest zoom level, then reads a session's
samples from the `samples` tiles covering its bounding box at `samples_zoom`. A sample's values
are attributes rather than its position, since a tile quantises a position to its grid. A sample
near a tile's edge also appears in the neighbouring tile's buffer, so a reader keeps one per
`seq`. `tippecanoe` drops a null attribute, so a reader takes a missing one as null.

The samples sit below the base map's lowest zoom level, so reading a session fetches no rail or
water.

## Where the base map comes from

The rail and water are the features the water crossings name, whole, across every country. Each
crossing names its rail segment and water feature by their Overture ids, and the extract it was
derived from. `pack_map` joins each country's crossings to that extract's segments and water on
both ids, so a feature is read from the extract its crossing came from. A crossing naming a
feature its extract lacks stops the pack.

## How the file is made

`pack_map` writes the features as newline-delimited GeoJSON, each with its layer and zoom range in
a `tippecanoe` member. [`tippecanoe`](https://github.com/felt/tippecanoe) tiles
them into an MVT PMTiles file in a temporary directory, keeping every feature at every zoom level.
[`mlt convert --profile web`](https://github.com/maplibre/maplibre-tile-spec) re-encodes that as
[MapLibre Tile](https://maplibre.org/maplibre-tile-spec/) into the output. Both tools are in
`just prerequisites`.

## Size

Over five sessions, 5,923 rail segments, and 5,329 water features, the file is 13.9 MB at maximum
zoom level 12. Each further zoom level adds about 60%. A client fetches single tiles through range
requests, so the total costs only the copy committed to git:

| Zoom level | Tiles | Median tile | Largest tile |
| --- | --- | --- | --- |
| 0–5 | 13 | 0.4 KB | 0.8 KB |
| 6 | 2 | 66.2 KB | 107.6 KB |
| 7 | 39 | 11.5 KB | 31.8 KB |
| 9 | 403 | 2.6 KB | 52.6 KB |
| 12 | 11,684 | 0.2 KB | 54.5 KB |
