# motis

Trains near where a device has been: poll a Motis server for the trips around recent GPS fixes,
keep every answer, and derive one row per scheduled leg from them.

[motis.md](../../docs/motis.md) covers what the server and the feed behind it can answer;
[medallion.md](../../docs/medallion.md) covers the layers the three stages write.

## The three stages

A **poll** reads the newest queued telemetry samples, keeps the GPS fixes younger than its
lookback, and holds them in a rolling window pruned by age. The box it queries is the window's
own bounding box scaled about its centre, so a train just off the trace still comes back. Nothing
is queried while the window is empty.

The **capture log** takes what a poll saw, one file per poll, verbatim: the times as instants and
the polyline as the encoded string the server sent. Nothing is rewritten, so overlapping polls each
keep their own view of a leg.

The **ingest** collapses those to one row per leg, the newest capture winning so that realtime
corrections survive, and decodes each polyline into geometry. A leg is written under the country it
starts in, since that fixes the zone its projected geometry is measured in; a leg starting outside
every country the store knows is counted and left unwritten.

## What counts as a train

Mainline and regional rail, by Motis's own modes: highspeed, long-distance, night, regional-fast,
regional and plain rail. Urban transit and road modes are dropped, so the capture is trains rather
than all transit.

A poll resolves the details a segment does not carry — the operating agency and the train number
— once per distinct trip, and caches nothing between polls: the server is local and a poll is
coarse. A trip whose lookup fails costs its row those two fields and nothing else.

## Testing against a server

One test drives a poll against a live Motis server at the default base URL, and one against a
mock; their names decide which profile runs which, as `.config/nextest.toml` sets out.
[`tools/motis-server`](../../../../tools/motis-server/Justfile) brings a server up.
