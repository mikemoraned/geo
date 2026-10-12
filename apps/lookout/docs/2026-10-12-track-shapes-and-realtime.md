# Track shapes and realtime data for Germany

*Assessment of 2026-10-12. No import has run with the setting it recommends, so no import time or
match rate has been measured. The evidence is Transitous's repository and API, the Motis source at
v2.11.3, and the DELFI feed of 2026-10-05. It also takes in the two Motis captures in bronze and one
snapshot of each German realtime feed. Whatever survives a first import belongs in
[motis.md](motis.md), and the rest goes stale.*

How a Motis server can give every German train a shape that follows the track, with realtime
corrections. It is for whoever next changes the local server in `tools/motis-server`, or resumes the
parked pfaedle slice in [next-slices.md](next-slices.md).

A shape is the line a trip is drawn along. `map/trips` returns it for each leg, as an encoded
polyline.

Transitous draws rail along the track because Motis computes the shapes during import, by matching
each route's stops to OpenStreetMap rail. No feed supplies them. The local server runs the same
Motis version as Transitous, v2.11.3, with the feature switched off. Switching it on needs no new
data source, no upgrade, and no change to the static feed. Realtime needs no new source either. The
local server already polls the trip-update feed Transitous uses for DELFI.

## Where Transitous's rail shapes come from

Transitous discards the shapes DELFI ships, and has Motis compute new ones during import. Two
settings do this:

- The DELFI source in `feeds/de.json` sets `drop-shapes`. Its fetch step therefore runs
  `gtfsclean --drop-shapes`, and the feed reaches Motis without `shapes.txt`.
- The import host adds a `timetable.route_shapes` block, with `mode: missing`, to the config before
  each import. The published `config.yml` lacks the block, because the update script inserts it
  with `sed`.

With no shape in the feed, `mode: missing` computes one for every DELFI route. Feeds without
`drop-shapes` keep the shapes they ship.

Transitous's API confirms the outcome. The `map/routes` endpoint reports the source of each route's
shape. `ROUTED` marks a shape Motis computed, and `TIMETABLE` one the feed supplied. One request over
Thuringia's bounding box, at zoom 8 and 23:36 UTC on 2026-10-11, counts these routes with a segment
inside the box:

| Mode | Shape computed by Motis | Shape from the feed |
| --- | --- | --- |
| High-speed rail | 1,561 | 0 |
| Long-distance rail | 720 | 0 |
| Regional rail | 2,245 | 21 |
| Night rail | 278 | 61 |

A route with no segment inside the box reports `NONE`, whatever its shape, so the counts leave it
out.

The captured legs show the difference in the shapes themselves:

| Capture | Rail legs | Median points per regional leg | Median points per high-speed leg |
| --- | --- | --- | --- |
| Local server, gtfs.de feed, 19–25 July 2026 | 60,007 | 2 | 3 |
| Transitous, DELFI feed, Thuringia, 2026-10-11 | 482 | 114 | 2,170 |

The local capture predates the server's move to DELFI, but DELFI's rail shapes are no finer. Every
DELFI trip carries a shape, but the rail shapes are straight lines between stops. They hold a median
of 2 points per leg, for regional, S-Bahn, and ICE trips alike.

## How Motis computes a shape

Motis computes shapes in an import step, `route_shapes`, that runs after the timetable and
street-graph steps. The step needs `timetable.with_shapes`, an `osm` file, and `street_routing`,
and the local config sets all three. It runs only for a config holding a `route_shapes` block, and
the local config holds none. The step's output files in the local data directory,
`routed_shapes_data.bin` and `routed_shapes_idx.bin`, are empty.

The step works per route, where a route is the set of trips sharing one stop sequence and one class.
For each route:

- The class picks a routing profile. Every rail class takes the railway profile: high-speed,
  long-distance, night, regional, suburban, subway, tram, and funicular. Bus and coach take the bus
  profile, and ships the ferry profile.
- The railway profile follows OSM ways tagged `railway=rail`, `light_rail`, `narrow_gauge`,
  `subway`, `tram`, `monorail`, or `funicular`. It uses yard, spur, and crossover tracks, and lines
  under construction, only at a penalty.
- Each stop matches to track within 200 metres. Where nothing lies that close, the radius widens to
  500 metres.
- Between consecutive stops, the shape follows the cheapest path through the track graph. Where no
  path joins two stops, the shape runs in a straight line between them, which the import log counts
  as a beelined segment.

The step writes only to Motis's shape storage. Trips, stops, and their ids stay as the feed has
them, so realtime updates resolve as before. Transitous shows both together: all 482 legs in its
Thuringia capture carry realtime times. `map/trips` draws each leg from the same storage, so
a client receives computed shapes with no change.

Five settings in the block decide what is computed:

| Setting | Effect |
| --- | --- |
| `mode` | `all` replaces every shape the feed ships. `missing` computes shapes for routes without one. |
| `clasz` | Turns classes off. A class turned off keeps the shapes its feed ships. |
| `max_stops` | Skips routes with more stops than the limit. |
| `n_threads` | Caps the threads, one per hardware thread by default. |
| `cache_reuse_old_osm_data` | Keeps cached shapes after the OSM file changes. |

A cache keyed by profile and stop coordinates carries shapes from one import to the next, so a
weekly timetable update computes only new routes. Unless `cache_reuse_old_osm_data` is set, a
changed OSM file empties the cache. Transitous sets it on six days a week, and recomputes every
shape on Sundays. The computed shape files grow with every import that adds routes. Deleting them
before an import makes Motis compute every shape again.

The step arrived in v2.9.0, released on 2026-03-19, and v2.11.3 carries the same settings.

## What the local server needs

Mode `missing` skips every DELFI rail route, since each already carries a shape. Transitous gets
past this by dropping the feed's shapes first. Mode `all`, with the classes outside the capture
turned off, recomputes the rail shapes and leaves the feed as DELFI ships it:

```yaml
timetable:
  with_shapes: true
  route_shapes:
    mode: all
    clasz:
      BUS: false
      COACH: false
      RIDESHARING: false
      ODM: false
      TRAM: false
      SUBWAY: false
      SUBURBAN: false
      SHIP: false
```

That leaves high-speed, long-distance, night, and regional routes to compute: the classes the
capture keeps. Bus and tram legs keep DELFI's shapes.

The setup recipe adds the realtime feed with `yq` after the import. The `route_shapes` block goes
in before the import instead, between `motis config` and `motis import`, since the import reads
it. The import of Germany takes 9 minutes 11 seconds without the step, and the step's cost is
unmeasured.

## Realtime

DELFI publishes Germany's realtime data as GTFS-RT on Mobilithek, under CC-BY-SA, matched to its
static feed. Transitous attaches three realtime feeds to its DELFI dataset:

| Feed | Content | Served by |
| --- | --- | --- |
| `https://germany.motis-project.org/gtfsrt` | Trip updates | A Motis server, v2.9.1 by its `server` header, re-publishing DELFI's GTFS-RT |
| `https://stc.traines.eu/mirror/german-delfi-gtfs-rt/latest.original.gtfs-sa.pbf` | Service alerts | A mirror of DELFI's GTFS-RT |
| A SIRI feed | Forecasts in SIRI ET | A host named only by an IPv6 address |

One snapshot of the trip-update feed, taken at 23:29 UTC on Sunday 2026-10-11, held 41,696 trip
updates. Of those, 40,651 were scheduled, 899 cancelled, and 146 added, with 185,969 stop-time
updates between them. The snapshot held no vehicle positions and no alerts, which matches
[motis.md](motis.md): no open German feed carries vehicle positions. The service-alert mirror held
362 alerts and nothing else.

The local server already polls the trip-update feed, so both servers share their main realtime
source. What the SIRI feed adds is unmeasured. Motis v2.11.3 accepts SIRI feeds as
`protocol: siri`.

## The pfaedle attempt, compared

The parked pfaedle slice matched rail to OSM outside Motis, and spliced the result into the feed's
`shapes.txt`. The shapes came out right, but realtime resolution fell to about 0.4% of trip
updates, for a reason never found. The `route_shapes` step leaves the feed alone, so Motis imports
the feed whose realtime resolves today.

The pfaedle slice's fallback, to "accept straight-line rail — which is what transitous does", no
longer describes Transitous.

pfaedle keeps one advantage. It prefers track that belongs to an OSM route relation carrying the
train's line. Its default config multiplies the cost of other track by 1.2. Motis takes the
cheapest path, which can follow the wrong one of two parallel lines. Motis issue #1596 reports a
case: RE2 trains between Rastatt and Karlsruhe drawn along line 4020 rather than 4000. The issue
proposes the same relation penalty, and is open.

## What a computed shape means for crossings

A computed shape is a model of the track, built from stop positions and OSM, not a record of where
the train ran. Two of its errors change which crossings a leg passes:

- A wrong parallel line swaps the crossings of one line for those of the other.
- A straight line between two stops, where matching failed, misses the crossings between them and
  can add false ones.

The import log counts the straight lines, which bounds the second error. The first needs a
comparison against recorded sessions on German lines.

## Coverage

The local server imports DELFI whole, so its rail covers every operator DELFI carries. Transitous
swaps some of DELFI's operators for the operators' own feeds. The swaps are mostly urban transit,
plus SNCF, SNCB, and Eurostar, which it takes from its French, Belgian, and European feeds.

## Approaches considered and rejected

**Polling Transitous across Germany.** Transitous already serves computed shapes and realtime. Its
usage policy asks for light use and contact before heavy use, and offers its source data in place
of scraping. Polls stay at one every 10 minutes over Thuringia, as agreed on 2026-10-06. A
Germany-wide capture at a shorter interval belongs on the local server.

**Importing Transitous's processed DELFI feed.** `api.transitous.org/gtfs/` serves it: 328 MB
against DELFI's 530 MB, with the shapes already dropped. Mode `all` achieves the same from DELFI's
own feed, without a dependency on Transitous's pipeline.

**pfaedle.** Parked. If Motis issue #1596 stays open and wrong parallel lines prove common, its
line-relation penalty is the reason to return to it.

**Computing shapes in lookout's own code.** Matching stops to OSM or Overture rail repeats what
Motis ships, and meets the same parallel-line problem.

## Not covered

- Vehicle positions. No open German feed carries them, so a train's position stays interpolated
  along its leg.
- Train names. Transitous's import script `de-DELFI.lua` formats DB Fernverkehr train numbers as
  `ICE 123`, the fix-up [motis.md](motis.md) describes. The local server can load such a script as
  a dataset `script`. The file is AGPL-3.0.
- Shapes for buses, trams, and the S-Bahn, which the capture leaves out.

## Open questions for a first import

- The time and memory the `route_shapes` step takes for German rail, and the size of the computed
  shape files.
- The routes matched, segments routed, and segments beelined, which the import log reports.
- Realtime resolution, against the 99.96% in [motis.md](motis.md).
- Agreement with Transitous for the same trips. Both servers' trip ids end in DELFI's own id, as in
  `…_de-DELFI_3432212213`, so captures from the two join on it. When both import the same weekly
  DELFI build, the join holds.

## References

- [Transitous `feeds/de.json`](https://github.com/public-transport/transitous/blob/95e54e95d041239ffa1341e3cb086bfa56fbc724/feeds/de.json)
  — the DELFI source, `drop-shapes`, and its realtime feeds
- [Transitous import script](https://github.com/public-transport/transitous/blob/95e54e95d041239ffa1341e3cb086bfa56fbc724/ansible/roles/motis/templates/motis-update-feeds-secondary.sh)
  — the `route_shapes` block, and the weekly cache refresh
- [Transitous `src/fetch.py`](https://github.com/public-transport/transitous/blob/95e54e95d041239ffa1341e3cb086bfa56fbc724/src/fetch.py)
  — the `gtfsclean` call
- [Transitous issue #2438](https://github.com/public-transport/transitous/issues/2438) — why
  Transitous matches with Motis rather than pfaedle at planet scale
- [Transitous API usage policy](https://transitous.org/api/)
- [Motis setup, Shapes](https://github.com/motis-project/motis/blob/v2.11.3/docs/setup.md#shapes)
  — the `route_shapes` settings and cache
- [Motis `src/route_shapes.cc` at v2.11.3](https://github.com/motis-project/motis/blob/v2.11.3/src/route_shapes.cc)
  — the step, its modes, and the class switch
- [Motis PR #1270](https://github.com/motis-project/motis/pull/1270) and
  [PR #1321](https://github.com/motis-project/motis/pull/1321) — the step's introduction, and its
  `mode` setting
- [Motis issue #1596](https://github.com/motis-project/motis/issues/1596) — trains drawn on the
  wrong parallel line, and the relation penalty proposed
- [osr `src/map_matching.cc`](https://github.com/motis-project/osr/blob/a7b2ec2728544304ef1d8397b3042abc8d10f7e7/src/map_matching.cc)
  and [`include/osr/extract/tags.h`](https://github.com/motis-project/osr/blob/a7b2ec2728544304ef1d8397b3042abc8d10f7e7/include/osr/extract/tags.h),
  at the commit v2.11.3 pins — the matching radii and the railway profile
- [pfaedle](https://github.com/ad-freiburg/pfaedle), its
  [default config](https://github.com/ad-freiburg/pfaedle/blob/master/pfaedle.cfg), and
  [Sparse map-matching in public transit networks with turn restrictions](http://ad-publications.informatik.uni-freiburg.de/SIGSPATIAL_Sparse%20map%20matching%202018.pdf)
  — the line-relation penalty
- [germany.motis-project.org licence](https://germany.motis-project.org/LICENSE.txt) — the sources
  behind the trip-update feed
- Mobilithek's [DELFI GTFS-RT](https://mobilithek.info/offers/858688352316981248) and
  [DELFI SIRI ET](https://mobilithek.info/offers/832259263033151488) offers
