# medallion

Paths and writers for the store [medallion.md](../../docs/medallion.md) describes. Every binary
that reads or writes it comes through here rather than joining strings, so the layer names, the
Hive layout and the rules a partition name must meet live in one place. What the layers are for is
that doc's; this is how the store is reached.

## A dataset is a value, and its layer is a type

A dataset is named once, as a spec carrying its name and its partition key, and passed around as
that value, so readers and writers agree on the layout by construction rather than by each
repeating a string. Whoever owns the data defines the datasets themselves, so this crate holds no
list of them.

The layer is the spec's type rather than a field. `DatasetSpec<layers::Bronze>` and
`DatasetSpec<layers::Silver>` are different types, and the operations that rewrite or delete are
implemented for the replaceable layers alone. Code that replaces a bronze partition does not
compile: the method is not there to call.

A dataset's columns are the row type declared beside it. A writer therefore states a schema by
naming its rows, and a reader of a dataset it did not write reads it back through the same type.
Instants are declared as UTC millisecond timestamps in that one place, so no two datasets can
drift apart on how time is stored.

## Appending never overwrites; deriving always replaces

The immutable layers append. A batch lands in a file named for the instant of the write, at
millisecond precision: a writer that batches — a drain, a backfill — issues several writes in
quick succession, and at second resolution the second would land on the first. A write that would
land on a file already there fails rather than replacing it, since the rows already written are
not this caller's to discard.

A derived partition is one file, whose name never varies, replaced whenever the partition is
derived again.

## A rebuild deletes what it no longer produces

A partition a run produces no rows for is deleted, not left standing. A derived dataset is built
wholesale from its source, so a partition left behind states something the derivation no longer
states, and a reader cannot tell it from a current one. A run therefore has to derive the whole of
what it sweeps: one covering some values of a key would read as a run that produced nothing for
the rest.

Every sweep is bounded by the key it is given, so nothing outside what the dataset itself writes
is ever removed, and a sweep of one level never reaches a partition of another. What a swept
dataset leaves is empty directories, which read as nothing written — the same as a dataset never
written.

## Two ways in, one implementation

A derivation written in Rust hands over rows of a row type; one written in another language hands
over an arrow table. Both go through the same layout, sweep and uniqueness checks, so which
language derived a dataset does not change what is stored.

Either way the caller supplies rows and the definition supplies the rest. The date a row is stored
under is read from the row itself, so a partition key and the column feeding it are paired where
the dataset is defined. The projected geometry is derived from the row's country rather than
supplied, since the zone a country's metres are in is the store's choice, and deriving it here is
what stops a caller projecting into one zone while the file declares another. Partition values are
read from the columns the layout names, and go into the path rather than the file.

Emptiness has to be said with a table rather than implied by silence. A query matching nothing
still writes a readable, correctly typed file, since a partition holding no rows is an answer. A
table of no rows is a derivation that produced nothing, and sweeps the dataset away; a call with
no batches at all carries no schema to check, and does nothing.

## Geometry, and the CRS it declares

Silver geometry is WKB simple features, with the CRS in the file metadata as PROJJSON. The
`geoparquet` encoder produces that metadata rather than this crate assembling it, so a file
conforms to the spec version that crate implements.

[One projected zone per country](../../docs/medallion.md#silver), chosen here, so a dataset states
which country's geometry it holds and never picks a zone of its own.

## Reading the store, and summarising it

A dataset is registered as a table by name, which walks its partition directories and reads its
geometry columns back with their CRS, so a caller expresses what it wants as a query rather than
as file traversal.

A summary answers "is this dataset there, and how much of it" without reading a row: the counts
come from each parquet file's own footer, so the cost is a seek per file rather than a scan.
Nothing in it interprets a dataset's columns, which is what lets one summary cover every dataset —
including those whose partitions hold different schemas, and so cannot be read as one table. A
dataset nothing has written is summarised as holding nothing, so the gaps show alongside the
contents.

## Where the store is

The default is [the store in the repo the caller is working in](../../docs/medallion.md#root),
found the way cargo finds a workspace. Every CLI takes the same `--medallion-root` to override it,
from one flag defined here.

A gold artefact is a file rather than a dataset, laid out by what it is and by the run that
produced it. Something outside the store holding one has no way to say which run that was, so a
rerun adds a version beside the last rather than replacing it. Wherever that version is named — a
build embedding an artefact, a deploy serving one — the same function the path uses spells it.
