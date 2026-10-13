# Motis and the German timetable

What the Motis server and the feed behind it can and cannot answer: the properties of the
external system, rather than of whatever asks it. Two servers answer here: Transitous, a public
service, and the local server, which [`tools/motis-server`](../../../tools/motis-server/Justfile)
sets up and runs.

## There is no vehicle position, from any German open feed

`GET /api/v6/map/trips` returns stop-to-stop legs (`TripSegment[]`) carrying mode, colour,
from/to places, scheduled and realtime times, and a Google-encoded polyline at precision 5. A
train's position at an instant is **interpolated** — walk the leg whose departure/arrival spans that
instant along its decoded polyline — and never a reported GPS position.

This is a property of the data, not of Motis. Both gtfs.de and DELFI publish GTFS-RT with
TripUpdates and ServiceAlerts and **zero VehiclePositions**, so realtime-corrected
interpolation is the nationwide ceiling. A second source of *real* positions has to come
from somewhere else.

Realtime therefore matters twice over: without it the interpolation is against the
scheduled timetable, and a delayed train is reported at its scheduled position.

## The feed is DELFI, and the choice changes what is answerable

DELFI's nationwide *Sollfahrplandaten* (`opendata-oepnv.de`, CC-BY-4.0), not the gtfs.de
free feed. What it gives that gtfs.de did not:

- **`route_type` is correct (101/102), so `mode` classifies.** Long-distance comes back as
  `HIGHSPEED_RAIL`/`LONG_DISTANCE`, regional as `REGIONAL_RAIL`. gtfs.de types all rail as
  `route_type=2`, so every train reports `REGIONAL_RAIL` and classification has to go via the
  agency. `mode` alone separates an ICE from an S-Bahn.
- **Train numbers exist, in `trip_short_name`.**

What it still does not give:

- **No IC/EC/ICE category anywhere in the feed.** `route_desc` and `route_long_name` are
  empty for 101/102, and `route_type` cannot split IC from EC. Synthesising a category
  prefix would mislabel EC trains, so the mode carries the family and the train number stays
  a bare integer.
- **Rail geometry is straight lines.** Every trip in the feed carries a shape, but rail shapes
  run straight from stop to stop. A rail leg comes back as a polyline of four points or fewer,
  where a coach leg has a thousand. Motis's `route_shapes` import step can compute rail shapes
  instead, by matching each route's stops to OpenStreetMap rail.

**An RT feed must match its static feed's ID namespace.** Pairing gtfs.de RT with a DELFI
static timetable makes around 99.9% of trip updates fail to resolve, because the trip and
stop ids differ. DELFI static with DELFI RT resolves 99.96%, and around 80% of segments in a
city-sized box come back realtime-corrected.

## Transitous also answers Germany from DELFI

[Transitous](https://transitous.org) runs Motis as a public service, at
`https://api.transitous.org`, on volunteers' servers. For Germany it answers from DELFI, with
DELFI's realtime feed, as the local server in
[`tools/motis-server`](../../../tools/motis-server/Justfile) does. It differs from the local
server, set up from DELFI alone, in three ways:

- **Rail legs follow the track.** Transitous drops the feed's shapes and computes each route's
  shape from OpenStreetMap rail with `route_shapes`.
- **Trains have names.** Its DELFI script formats a train's name, such as `ICE 123`.
- **Realtime adds forecasts.** It adds DELFI's SIRI ET forecasts to the trip updates.

Its usage policy asks two things of a caller:

- Every request carries a `User-Agent` naming the project, its version, and where to reach its
  maintainer.
- Anything showing Transitous data links to https://transitous.org/sources/, which credits the
  feeds.

A poll of Thuringia costs one `map/trips` request, plus one `trip` request per train not yet looked
up in the run. The first poll of a run made 52 `trip` requests.

## Zoom selects modes, not detail

`map/trips` takes a zoom, and raising it widens what comes back rather than refining it: urban
transit — subway, tram, bus — appears on top of the long-distance and regional rail a low zoom
answers with. A query therefore asks for the modes it wants by the zoom it sends, and filters the
rest out of the answer.

## A whole country fits one request

One `map/trips` request over Germany's bounding box, at zoom 8 and a ten-minute window, returns
7.1 MB in 0.23 s from the local server. Measured once, at 09:00 on a weekday: 7,146 legs from 2,939
trips, 3,594 of the legs rail. A poll covering the country therefore needs no tiling.

## `map/trips` answers a bounding box, not an area

The request takes a bounding box alone. A capture of an area, such as a country or a region within
one, asks for the bounding box of the area's outline. It then keeps the legs whose polyline touches
the outline, and counts the rest as outside. The outline comes from Overture's `division_area`. In
one poll of Thuringia, 108 rail legs were kept and 114 fell outside.

Testing every polyline against a detailed outline is slow unless the outline is indexed. For one
poll of Thuringia in a release build, the unindexed test takes 54 s and the indexed one 0.1–0.25 s.

## Train number and agency need a second call

`map/trips` carries neither. Its `TripInfo` exposes `routeShortName` — the bare line, `"55"`
— and a `displayName` that is null on raw DELFI, since the formatted `IC 2569` is a
downstream Lua fixup rather than feed content.

`GET /api/v6/trip` answers both, as an itinerary of legs carrying agency and
`trip_short_name`. Query it with interlined legs unjoined: a stay-seated trip otherwise
collapses into one leg spanning multiple agencies, and the answer becomes whichever agency
came first.

## A segment carries one trip

A `map/trips` segment lists its trips as an array, and the spec does not describe it. The server
fills that array with exactly one trip: v2.11.3 builds each segment in `src/railviz.cc` with a
single `TripInfo`. Of 2,432 rail segments in one country-wide response, every one carried one
trip. A segment carrying none, or several, is therefore an error rather than a choice.

## A leg's identity is `(trip_id, from_stop_id, departure)`

Timetables are minute-resolution, and two legs of one trip can depart *different* stops
within the same minute. Keying on `(trip_id, departure)` therefore drops legs silently, with
nothing to indicate it.

## The server reports its version, not its feed

`map/initial` names the server's Motis version in `serverConfig.motisVersion`. No endpoint names
the feed it answers from, so a caller states the feed itself.

## A capture names the server that answered it

Each captured leg carries the id of its source: one Motis server answering from one feed. These
fields describe a source:

- the server's base URL
- its Motis version, from `serverConfig.motisVersion`
- the feed, `local` or `transitous`, as the caller states it
- the area captured, if any: a country, an optional region, and the Overture release the area's
  outline came from
- an optional label

A region is recorded as its Overture division id, which joins to the extract, and its name, for a
reader. The label tells apart two setups of one server that the other fields cannot, such as two
DELFI builds. It holds lowercase letters and digits, with a single `-` or `.` between words, such
as `gtfs.de` or `delfi-2026-10-12`.

The id is a UUID v5 of those fields, so one source always gets the same id. A change to any field,
such as a Motis upgrade or a new Overture release, makes a new source. A label adds to the hash
only when present.

Bronze `motis_source` holds one row per source, unpartitioned, and recording a source already
present writes nothing. Bronze `motis_segment_v2` holds the captured legs, partitioned by
`polled_date`, with one file per poll. A row is one leg seen in one poll: the trip, its train
number and agency, the mode, both stops, the scheduled and realtime times, and the polyline.

The earlier captures in `motis_segment` carry no source. Silver `train_segment_v2` derives from
`motis_segment_v2`, and `train_segment` from `motis_segment`.

## Two server quirks a caller has to absorb

Both are Motis behaviours rather than ours, and both look like an empty or nonsensical
result rather than an error: Motis binds IPv4 only, so `localhost` resolving to `::1` never
connects; and `map/trips` mis-parses time bounds carrying fractional seconds, swinging
between empty and wildly oversized responses. A request is built around each: an explicit
IPv4 host, and time bounds truncated to the second.
