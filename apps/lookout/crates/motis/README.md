# motis

Trains near where a device has been: poll a Motis server for the trips around recent GPS fixes,
keep every answer, and derive one row per scheduled leg from them.

[motis.md](../../docs/motis.md) covers what the server and the feed behind it can answer;
[medallion.md](../../docs/medallion.md) covers the layers the three stages write.

## The three stages

A **poll** has two parts: one decides where to look, and the other captures what Motis shows
there. Near recent GPS, the first reads the newest queued telemetry samples, keeps the GPS fixes
younger than its lookback, and holds them in a rolling window pruned by age. The area it names is
the window's own bbox scaled about its centre, so a train just off the trace still comes back.
While the window is empty it names no area, and nothing is captured. The capture queries Motis
over the area it is given, whatever decided it.

The **capture log** takes what a poll saw, one file per poll, verbatim: the times as instants and
the polyline as the encoded string the server sent. Nothing is rewritten, so overlapping polls
each keep their own view of a leg.

The **ingest** collapses those to one row per leg, the newest capture winning so that realtime
corrections survive, and decodes each polyline into geometry. A leg is written under the country
it starts in, since that fixes the zone its projected geometry is measured in; a leg starting
outside every country the store knows is counted and left unwritten.

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
