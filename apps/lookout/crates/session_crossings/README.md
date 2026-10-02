# session_crossings

The ground truth: which water crossings each recorded session actually passed. It joins what two
other crates produce — the sessions derived from telemetry, and the crossings derived from
reference data — so it sits beside both rather than inside either.
[medallion.md](../../docs/medallion.md) describes the store it reads and writes.

## A crossing is passed when a sample comes near it

The rule is pure distance: a crossing was passed in a session if any sample came within the match
radius, and the nearest sample says when. It is deliberately simple, with a known failure — a
crossing on a line running parallel to the one travelled is within the radius, so it is recorded
as passed though it never was. Fixing that means matching a session to track rather than to
points, which is work in its own right.

[Two numbers travel with each match so a reader can weigh
it](../domain/README.md#what-was-found-and-how-hard-something-looked-are-different-things): one
sample within the radius and twenty are different evidence, and a session that never moved can
produce the first without having gone anywhere.

The default radius is where the nearest-sample distances stop looking like crossings that were
passed. Their distribution has two parts: a decay from zero, which is a crossing actually gone
over seen from however far the previous fix happened to be — a train at 100 km/h sampled every ten
seconds leaves 280 m between fixes — and beyond it a flat spread, which is the density of
crossings near a path rather than on it. The default is where the decay ends, and the evidence for
the value is in the slice record.

## Distances are metres, so the work runs a country at a time

A distance is only a distance inside one projected zone, and the store chooses a zone per country,
so both inputs are read a country at a time and subtracted in that country's metres. A session is
matched only against the crossings inside its own bbox grown by the radius, so the distance is
computed for the pairs that could be within it rather than for every pair. The bbox is grown
on the sphere rather than by treating a degree as a fixed distance, since a degree of longitude is
a different length at every latitude. Its edges count as inside: a crossing on the grown edge is
one at exactly the radius, which the distance test counts.

A match is a session, a crossing and an instant, with no geometry of its own, so the rows are
partitioned by the date it happened and nothing else. A run derives the whole dataset from the
whole of silver, so a partition it no longer produces rows for goes with it.

What a stored row adds to a pass is the device and the radius. The device is derivable from the
session and carried anyway, so a partition reads without joining back — as the samples carry it
for the same reason. The radius is the run's own: a match made at 150 m and one made at 20 m are
not the same claim, so a row stays interpretable after the default changes.

## Choosing what a kiosk replays

A session is worth replaying if it passes crossings, since one that passes none shows an empty
screen for as long as it runs. That is already derived — a row per session per crossing passed —
so choosing is a count over those rows rather than a second pass over the geometry: keep the
sessions passing at least so many, take the best few, and write each with the samples that replay
it.

One query reads the samples of the chosen few and groups them in memory, rather than a query per
session: the chosen are a handful, and the samples are one scan either way.

A coordinate is written to the six places [an export keeps](../../docs/medallion.md#gold), and
every other reading to one, about a tenth of a metre.
