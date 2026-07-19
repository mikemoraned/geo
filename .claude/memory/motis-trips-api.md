# Motis map/trips API (lookout "second source from Motis" slice)

Endpoint `GET /api/v1/map/trips` (canonical `/api/v6/`; server is version-tolerant).
Required params: `zoom`, `min="lat,lon"` (SW corner), `max="lat,lon"` (NE corner),
`startTime`/`endTime` (RFC3339); optional `precision`. Returns `TripSegment[]`
(stop-to-stop legs: `trips[]`, `mode`, `routeColor?`, `from`/`to` Place,
`departure`/`arrival`/`scheduled*`, `realTime`, Google-encoded `polyline`).

Constraints found by research (2026-07-19):
- **No vehicle-positions endpoint.** `map/trips` gives *interpolated* position (walk the
  segment whose `[departure,arrival]` spans T along the decoded polyline), never a raw
  GPS dot.
- **Realtime must be enabled** on the Motis server: add the gtfs.de free RT feed
  (`https://realtime.gtfs.de/realtime-free.pb`, `protocol: gtfsrt`) under the
  `germanygtfs` dataset's `rt:` in `tools/motis-server/motis_server/config.yml`, then
  restart `motis server` (no re-import; polled every `update_interval`=60s). Then
  `map/trips` returns `realTime:true` with delay-corrected `departure`/`arrival`.
- The free German feed is **TripUpdates + ServiceAlerts only — no VehiclePositions** — so
  the best available is realtime-*corrected interpolation*, nationwide, not real GPS.
- **Polylines are straight stop-to-stop lines** because the gtfs.de free feed ships no
  `shapes.txt` (verified: absent from the zip, no `shape_id` in `trips.txt`), so Motis has
  only stop coordinates. Motis does NOT auto-generate shapes from OSM today (on their TODO;
  `with_shapes: true` + loaded OSM still gives straight lines). Fix: run `pfaedle` (rail
  map-matcher) over `germany.osm.pbf` + `germany_gtfs.zip` to generate `shapes.txt`
  (`pfaedle -x germany.osm.pbf -m rail -X filtered.osm.pbf germany_gtfs.zip`), repackage,
  re-import Motis → real track geometry.

Decisions: depend on the `motis-openapi-progenitor` crate (0.4.0, progenitor-generated,
reqwest 0.12, `.trips()` + `types::TripSegment`) rather than hand-write/generate in-repo;
decode polylines with the `polyline` crate.

See [lookout-architecture](lookout-architecture.md).
