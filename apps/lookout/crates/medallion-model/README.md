# medallion-model

Every dataset lookout holds, defined once: its layer, its partition key, and its columns.
[medallion.md](../../docs/medallion.md) describes the store's layout in prose, and this is that
layout in code. A writer and a reader of one dataset therefore agree by referring to the same
definition, rather than each spelling out a name, a key and a struct of its own.

Everything here is the store's, and all of it depends on arrow. What a device and the store both
hold is [`domain`](../domain/README.md).

## A dataset is a spec and a row type

The columns of a dataset are a row type declared beside its spec. Geometry is the exception: a
geometry column is arrow the writer builds rather than a traced Rust field, so a row type declares
the other columns and the writer appends the lat/lon and projected ones.

The list of all of them holds summaries rather than the specs themselves, since a spec carries its
layer in its type and datasets of different layers cannot sit in one array.

A caller that cannot hold a Rust row type — a table built in another language — names a silver
dataset instead, and the name resolves to the same definition a Rust writer uses. Only silver
datasets can be named that way, because writing a table replaces what it writes, and silver is the
layer a derivation may replace.

## The bronze datasets: what arrived

Every telemetry payload lands verbatim in `raw_sample`, keyed on its md5, and the readings
interpreted from it go to one dataset per sensor. `device_session` holds what a device said about
itself as recording began.

`motis_segment` holds trip segments as polled, duplication allowed, with the times as instants and
the path as the encoded polyline the service sent — bronze records what arrived rather than a
normalised form of it.

`overture_extract` holds upstream reference rows in the upstream's own shape and layout, so those
rows have no row type here: they keep whatever columns the release gives them, plus the
`extract_id` joining them to `extract_manifest`.

The manifest is one row per extraction, and its window is four bounds — `xmin`, `ymin`, `xmax`,
`ymax` — rather than a geometry column in a Simple Features encoding. It is provenance, answering
"what was this restricted to", and comparing numbers settles that. It is also the shape the upstream
uses: [an extracted row](../../docs/overture.md#every-row-carries-its-own-envelope) carries a bbox
of the same four bounds beside its own geometry, so a reader of the manifest and a reader of the
extract see one thing.

## Sessions keep every sample and flag the doubtful ones

What counts as a usable sample is a threshold of whoever is reading — a ground truth and a
predictor are entitled to disagree — so these datasets carry the columns a filter needs and leave
the line to the consumer. `implied_speed_mps` is the clearest case: a value far above what the
vehicle could do marks a bad sample, and the column reports it rather than the store acting on it.
It is absent on a session's first sample, which has no previous one to step from.

A session is closed only in the sense that no later sample has been recorded yet, so `ended_at` on
the most recent one moves as more arrive. `seq` counts a sample's place in its session from zero,
and a sample is identified by `(device_id, t)`, the identity it was deduped from bronze on. A
sample carries `device_id` as well as its session's id, so a partition of samples reads without
joining back.

## How a crossing was collapsed is stored on the row, because the collapse defines it

A crossing is the collapsed representative of the parts a stretch of track and one body of water
overlap in — a bridge is one crossing, not one per span — so a row carries its own overlap, the
total over the parts merged into it, and how many parts those were. `frac` says where along the
named segment it sits, from 0 at the start to 1 at the end. `track_id` is the connected stretch of
physical track, named canonically from its own members rather than by a label a run assigned;
`rail_id` is the upstream segment the representative part lies on, one of possibly several making
up the track.

The columns saying how close two parts had to be to merge, and the shortest overlap the run kept,
travel with the row. This is deliberately unlike the sessions: there a threshold belongs to
whoever is reading, whereas two runs that collapsed differently do not agree on what a crossing
*is*, and a ground truth and a prediction that count different things cannot be compared. Changing
how the collapse works therefore means rebuilding the dataset.

A crossing carries both of its names, and each identifies it on its own: the store's, and the four
bytes a device holds instead. A compact id shared by two crossings is one a device could not tell
apart, so it is refused here rather than at the buffer that packs it.

## What the tests hold steady

Tracing the schema from Rust types means a change to a type changes the stored columns, so the tests
assert the properties a reader already depends on rather than the schema itself: that a dataset's
identity cannot collide or drift, that only what the store permits to be replaced is replaceable,
that a column a row type calls an instant really is one, and that the values readers join and filter
on keep the type they are stored as. Each test is named for the property it holds, so a failure
reads as the rule that broke.
