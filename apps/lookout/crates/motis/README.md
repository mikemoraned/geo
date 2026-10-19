# motis

Trains near where a device has been, or across one country or region: poll a Motis server for the
trips there, keep every answer, and derive from them one row per scheduled leg and one train
session per trip. `motis_follow_gps` polls around recent GPS fixes, and `motis_track_area` polls
one Overture country or region.

[motis.md](../../docs/motis.md) covers what the server and the feed behind it can answer;
[medallion.md](../../docs/medallion.md) covers the layers the three stages write.

## The three stages

A **poll** has two parts: one decides where to look, and the other captures what Motis shows
there. Near recent GPS, the first reads the newest queued telemetry samples, keeps the GPS fixes
younger than its lookback, and holds them in a rolling window pruned by age. The area it names is
the window's own bbox scaled about its centre, so a train just off the trace still comes back.
While the window is empty it names no area, and nothing is captured. Over a fixed country or
region, the first names the same area at every poll: its Overture shape. The capture queries Motis
over the bounding box of the area it is given, whatever decided it, and keeps the legs that touch
the area.

The **capture log** takes what a poll saw, one file per poll, verbatim: the times as instants and
the polyline as the encoded string the server sent. Nothing is rewritten, so overlapping polls
each keep their own view of a leg.

The **ingest** collapses those to one row per leg, the newest capture winning so that realtime
corrections survive, and decodes each polyline into geometry. A leg is written under the country
it starts in, since that fixes the zone its projected geometry is measured in; a leg starting
outside every country the store knows is counted and left unwritten.

## A train as a session

The ingest also derives one train session per trip, from the same collapsed legs. A train session
is shaped as a recorded session is, so whatever replays a recorded session replays a train too. Its
samples simulate a device recording on board the train: one every 10 s, as a device
[sends them](../../docs/telemetry.md).

Each sample's `source` says where its position came from:

- `Stop`: the train at a stop, at a leg's departure or arrival there. A poll states both directly.
- `Interpolated`: every other sample. Between a departure and the next arrival, the train moves
  along the leg's shape at the leg's own speed: the shape's length over the time from departure to
  arrival. Between an arrival and the next departure, it waits at the stop.

A sample falling on a departure or an arrival is one sample, a `Stop`. Each leg's times are its
newest capture's, realtime where Motis had it. Motis gives them to the whole minute, which bounds
how closely any sample places the train.

A train session is named by a UUID v5 of its source and its trip, and carries both ids where a
recorded session carries its device. It leaves out what only a device reports: altitude,
accuracy, speed, heading, and how the recorder opened the session.

## What counts as a train

Mainline and regional rail, by Motis's own modes: highspeed, long-distance, night, regional-fast,
regional and plain rail. Urban transit and road modes are dropped, so the capture is trains rather
than all transit.

A segment does not carry its operating agency or its train number, so the poller asks the server
for them. It asks once per trip per run, and every later poll seeing that trip reuses the answer.
A remote server sees one request per train rather than one per poll. A trip whose lookup fails is
still captured without those two fields, and is asked about again at the next poll. A segment
without exactly one trip is not captured. Each poll reports both kinds of failure, and the poller
logs them as errors.

## The client

The client is generated at build time from `openapi.json`, the API spec of the Motis version
[`tools/motis-server`](../../../../tools/motis-server/Justfile) pins. After a Motis upgrade,
`just motis-openapi` fetches the matching spec, and the next build regenerates the client from it.

## Testing against a server

One test drives a poll against a live Motis server at the default base URL, and one against a
mock; their names decide which profile runs which, as `.config/nextest.toml` sets out.
[`tools/motis-server`](../../../../tools/motis-server/Justfile) brings a server up.
