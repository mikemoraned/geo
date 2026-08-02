# Motis and the German timetable

What the Motis server and the feed behind it can and cannot answer. The client that asks is
`crates/motis`, whose own doc comments carry the implementation; this records the properties
of the external system that shaped it. Running the server is
[`tools/motis-server`](../../../tools/motis-server/Justfile).

## There is no vehicle position, from any German open feed

`GET /api/v1/map/trips` returns stop-to-stop legs (`TripSegment[]`) carrying mode, colour,
from/to places, scheduled and realtime times, and a Google-encoded polyline. A train's
position at an instant is **interpolated** — walk the leg whose departure/arrival spans that
instant along its decoded polyline — and never a reported GPS position.

This is a property of the data, not of Motis. Both gtfs.de and DELFI publish GTFS-RT with
TripUpdates and ServiceAlerts and **zero VehiclePositions**, so realtime-corrected
interpolation is the nationwide ceiling. A second source of *real* positions has to come
from somewhere else.

Realtime therefore matters twice over: without it the interpolation is against the
scheduled timetable, and a delayed train is reported where it should have been.

## The feed is DELFI, and the choice changes what is answerable

DELFI's nationwide *Sollfahrplandaten* (`opendata-oepnv.de`, CC-BY-4.0), not the gtfs.de
free feed. What it gives that gtfs.de did not:

- **`route_type` is correct (101/102), so `mode` classifies.** Long-distance comes back as
  `HIGHSPEED_RAIL`/`LONG_DISTANCE`, regional as `REGIONAL_RAIL`. gtfs.de typed all rail as
  `route_type=2`, so every train reported `REGIONAL_RAIL` and classification had to go via
  the agency. `mode` alone now separates an ICE from an S-Bahn.
- **Train numbers exist, in `trip_short_name`.**

What it still does not give:

- **No IC/EC/ICE category anywhere in the feed.** `route_desc` and `route_long_name` are
  empty for 101/102, and `route_type` cannot split IC from EC. Synthesising a category
  prefix would mislabel EC trains, so the mode carries the family and the train number stays
  a bare integer.
- **Rail geometry is straight lines.** `shapes.txt` is present and Motis loads it, but only
  bus and coach operators populate it — rail legs come back as polylines of four points or
  fewer, where a coach leg has a thousand. Curved rail needs shapes synthesised by
  map-matching against OSM, which is parked; see the pfaedle slice in
  [next-slices.md](next-slices.md).

**An RT feed must match its static feed's ID namespace.** Pairing gtfs.de RT with a DELFI
static timetable makes around 99.9% of trip updates fail to resolve, because the trip and
stop ids differ. DELFI static with DELFI RT resolves 99.96%, and around 80% of segments in a
city-sized box come back realtime-corrected.

## Train number and agency need a second call

`map/trips` carries neither. Its `TripInfo` exposes `routeShortName` — the bare line, `"55"`
— and a `displayName` that is null on raw DELFI, since the formatted `IC 2569` is a
downstream Lua fixup rather than feed content.

`GET /api/v4/trip` answers both, as an itinerary of legs carrying agency and
`trip_short_name`. Query it with interlined legs unjoined: a stay-seated trip otherwise
collapses into one leg spanning multiple agencies, and the answer becomes whichever agency
came first.

## A leg's identity is `(trip_id, from_stop_id, departure)`

Timetables are minute-resolution, and two legs of one trip can depart *different* stops
within the same minute. Keying on `(trip_id, departure)` therefore drops legs silently, with
nothing to indicate it.

## Two server quirks the client absorbs

Both are Motis behaviours rather than ours, and both look like an empty or nonsensical
result rather than an error: Motis binds IPv4 only, so `localhost` resolving to `::1` never
connects; and `map/trips` mis-parses time bounds carrying fractional seconds, swinging
between empty and wildly oversized responses. `crates/motis/src/client.rs` handles each and
explains it at the point of the fix.
