# recorder

Everything between a queued sample and a session on the map: drain the queue into bronze, split
the samples into sessions, write those to silver. [The telemetry wire](../../docs/telemetry.md)
describes what a queued sample is; [medallion.md](../../docs/medallion.md) describes the layers
written here.

## Draining in batches bounds what a failed write can lose

Two modes, the non-destructive one by default: peek at the newest samples and archive them,
leaving the queue as it was, or drain it — take the oldest sample repeatedly until the queue is
empty. The queue's own verbs are [`telemetry`](../telemetry/README.md)'s: a take removes, a peek
does not, and a drain is the loop over takes.

A drain writes in bounded batches rather than once at the end. Taking a sample off the queue has
already removed it, so a batch that fails to write is put back — and if that requeue also fails,
the samples are gone. Batching caps how many a single failure can put at risk, however long the
drain runs.

Every payload is archived verbatim, whether or not anything can interpret it, so the archive stays
the record everything else derives from. A reported fix carrying no accuracy is malformed rather
than partial, since every browser reports one and every recording holds one. Nothing is interpreted
from such a payload: it lands in the archive and in no reading dataset, which is where a payload no
version can parse lands too.

## A session is a contiguous run of samples from one device

A run derives every boundary from the whole of bronze rather than from what has arrived since the
last run. The newest session is always still open — more of its samples arrive with the next drain
— so a run has to re-derive a session it has already written and reach the same answer.

Bronze tolerates the same observation arriving twice, so samples are deduped before anything looks
at the intervals between them: a repeated sample left in place is a zero-length interval, which is
not a silence and must not be read as one. Duplicates collapse to the first row of a total order
over the values reported, rather than to an arbitrary one — two rows sharing an instant but
disagreeing on what was measured would otherwise let a rerun pick differently and derive different
sessions from the same bronze.

### What starts a session

A device reporting that it has begun recording starts a session. That is explicit, so it outranks
both of the inferred reasons. Failing a report, the first sample ever seen from a device starts one
— as it must, since the earliest protocol version could not report a start at all — and after that
a long enough silence starts each of the rest.

The silence threshold is long enough that a stop at a station, a signal or a tunnel does not end a
journey, and short enough that two journeys either side of an errand are not read as one. A
silence of exactly the threshold does not separate: it is the longest silence a session survives.

A report also reaches backwards. A device fixes its position before it announces that it is
recording, so the first samples of a journey can arrive seconds ahead of the announcement; left
alone they open a session of their own, and the announcement starts a second one moments later,
splitting one journey in two. So a session that began within the lead window of a report is the
same journey, and its samples open the announced session instead.

The whole session has to fall inside that window, not merely its last sample: one that has been
running longer is a journey in its own right, however soon an announcement follows it. The window
sits well above the observed spread — a device announces within seconds of its first fix — and far
below the silence threshold, so it can only absorb what a silence had just separated.

A session carries both thresholds, so one split under a pair of them stays interpretable after the
defaults change. It holds at least one sample: the samples are what make it, so a reported start
that nothing follows is no session.

## Silver carries every geometry twice

Once in lat/lon, and once projected into metres. The projected one is what makes a distance a
distance: an implied speed is metres per second, and degrees are neither metres nor the same size
in both axes. A sample's implied speed is therefore measured between projected positions, and the
interval is never zero, since the samples were deduped before they got here.

Which country a session is in follows from where it started, and that fixes the zone its projected
geometry is measured in — for its samples as much as for itself, so a session and the samples
making it up are measured in the same metres wherever they later went. A session starting outside
every country the store knows has no zone to project into, so it is counted and left unwritten.

A session and its samples are partitioned by different dates — a sample by its own instant, a
session by the instant it began — so a session crossing midnight has its samples split over two
partitions while itself living in one.

A session of one sample stands still rather than having no path: its lone point is repeated, so
every session's geometry is a line of at least two coordinates and no reader meets a `LineString`
that simple features would call malformed.
