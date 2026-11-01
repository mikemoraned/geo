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

## A poll asks for its own interval

`map/trips` takes a time window beside the bounding box, and returns the legs running within it.
Each poll asks for a window as wide as the interval between polls, centred on the poll's instant.
Consecutive polls therefore cover time with no gap and no overlap.

## `map/trips` answers a bounding box, not an area

The request takes a bounding box alone. A capture of an area, such as a country or a region within
one, asks for the bounding box of the area's outline. It then keeps a leg when its polyline
touches the area and lies wholly within the area's country, and counts the rest as outside. For a
country, the area and the country are one outline. Each outline is the union of its
`division_area` rows in Overture, so a leg crossing from a country's land onto its territorial
waters stays within it. A capture around a point, such as recent GPS, keeps every leg touching
its bounding box.

A leg that leaves the country is cut from the capture because the local server routes over one
country's OSM extract. Beyond the border it has no track, and draws the leg straight. A leg that
leaves a region but stays in the country runs on track throughout, so it stays. In one poll of
Germany, 417 rail legs were kept and 18 fell outside. In one poll of Thuringia, 16 were kept and
11 fell outside.

Testing every polyline against a detailed outline is slow unless the outline is indexed. For one
poll of Thuringia in a release build, the unindexed test takes 54 s and the indexed one 0.1–0.25 s.

## A leg's shape can lack a second point

With `route_shapes`, a leg's polyline can decode to a single point, the start of the leg. One poll
of Germany held such a leg: two stops 120 m apart at Niebüll, seven minutes apart in the timetable.

The encoding's precision does not cause it. Precision 5 resolves about a metre, and the two stops
lie about 185 steps apart. The encoding keeps a repeated point as a zero step, so it never merges
points either. The server emits the one point.

A line of one point is not a valid geometry, and GEOS refuses to read one. The capture keeps the
polyline as it arrived. Deriving silver, a leg whose shape has fewer than two distinct points takes
the straight line between its two stops. A leg whose stops share one position has no line at all,
and is counted rather than written.

## A leg's polyline can be empty or invalid

The API requires a leg's `polyline` field, so a leg with no polyline arrives with empty text. The
area test needs a line, so a capture leaves such a leg out. It counts these legs and logs a warning.

An invalid polyline is text the decoder refuses, or text with a character outside ASCII 63–126. A
capture ignores that leg's row, logs an error, and counts it. The poll's other legs still land.

The `polyline` 0.11 decoder checks only a character's lower bound. It reads a byte of 127 or above
as part of a coordinate, and can return a line with wrong coordinates. A polyline's character check
therefore runs alongside decoding.

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

## A leg's identity is its source, trip, departure stop, and scheduled departure

A leg is identified by the source that captured it, its trip id, the stop it departs, and its
scheduled departure. A delay moves the realtime departure and leaves the scheduled one, so the
polls that see a delayed leg see one leg, and the newest capture of it is kept.

Timetables are minute-resolution, and two legs of one trip can depart *different* stops within
the same minute. A trip and a departure time alone therefore drop legs silently, with nothing to
indicate it. A trip that departs one stop twice keeps both legs, since their scheduled departures
differ.

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

A country is recorded as its ISO 3166-1 alpha-2 code, such as `DE`. The code is not checked against
the countries the store defines, so a source naming a country the store has no zone for still reads.
A caller names a region by its ISO 3166-2 code, such as `DE-TH`. The region is recorded as its
Overture division id, which joins to the extract, and its name, for a reader. The label tells apart
two setups of one server that the other fields cannot, such as two DELFI builds. It holds lowercase
letters and digits, with a single `-` or `.` between words, such as `gtfs.de` or `delfi-2026-10-12`.

The id is a UUID v5 of those fields, so one source always gets the same id. A change to any field,
such as a Motis upgrade or a new Overture release, makes a new source. A label adds to the hash
only when present.

Bronze `motis_source_v4` holds one row per source, unpartitioned, and recording a source already
present writes nothing.

A poll lands in `motis_poll_v2`, partitioned by `polled_date`, with one file per poll. A row is one
leg seen in one poll: the trip, its train number and agency, the mode, both stops, the scheduled
and realtime times, and the polyline.

`just bronze-promote-motis-landing` drains landing into two bronze datasets:

- `motis_polyline_v2`, unpartitioned, holds each polyline once, under a UUID v5 of its text. A drain
  adds one file holding the polylines bronze lacks.
- `motis_segment_v3`, partitioned by `polled_date`, holds a row for each leg in each poll, with a
  polyline id in place of the polyline. A drain adds one zstd file per date. Rows sort by stop pair,
  polyline id, trip, and capture time.

A drain interrupted before it deletes its landing files writes those polls again on the next run,
and deriving silver collapses the repeat.

The earlier captures in `motis_segment` carry no source. Two sources stand for them, both the
local server at Motis v2.10.2 with no area: one labelled `gtfs.de`, for the captures before the
server moved to DELFI on 23 July 2026, and one labelled `delfi`, for those from then on. Silver
`train_segment_v4` derives from both capture datasets, and gives each earlier capture the source
its capture date falls under.

Silver derived from either capture dataset holds only the rail modes a capture keeps. Deriving the
segments leaves out a leg of any other mode, such as a ferry in the earlier captures, and counts it.

## Polyline text dominates a capture

Polling Germany every 10 minutes repeats each polyline about 80 times. A leg appears in every poll
whose window it overlaps, and many trips share the polyline between two stops. In 1,919,542 rows,
polyline text takes 1,334.5 MB, 87% of a dataset holding it in every row. Only 24,790 polylines are
distinct, 16.7 MB in all. Few are new after the first full day: 161 and 117 on the fourth and fifth.

Sizes over three days of about 1.17 million rows, per day:

| Layout | On disk | In git |
| --- | --- | --- |
| A file per poll, uncompressed, polyline in every row | 304.6 MB | 58–70 MB |
| A file per poll, zstd, polyline in every row | 160 MB | 160 MB |
| A file per date, zstd, polyline stored once | 5.65 MB | 5.4 MB |

These sizes leave out the polyline table, which grows by 0.10–0.27 MB a day after its first full
day. DuckDB wrote the files behind the second row, which covers one day, and behind the third row's
size in git. Git shrinks a zstd file by 4% at most.

Sort order sets the size of a merged file. In a file per day sorted by stop pair, each coordinate
column takes 0.09–0.14 MB. Sorted by trip, each takes 0.73–0.75 MB.

Storing only a leg's changes gains little. Over the same days, 35% of rows repeat another capture of
the same leg, and a sorted file already stores those cheaply. A row would also change meaning, from
one leg in one poll to one leg over a span of polls.

## Two server quirks a caller has to absorb

Both are Motis behaviours rather than ours, and both look like an empty or nonsensical
result rather than an error: Motis binds IPv4 only, so `localhost` resolving to `::1` never
connects; and `map/trips` mis-parses time bounds carrying fractional seconds, swinging
between empty and wildly oversized responses. A request is built around each: an explicit
IPv4 host, and time bounds truncated to the second.
