# summary

What a store holds, as text meant to be read at a glance: a line per dataset, headed by the
layer it lives in, with the gold artefacts last. The `summarise` binary asks it of every dataset
this app defines.

## The layout is a column per question

Is it there? How much of it? What span does it cover? The measurements are right-aligned so their
magnitudes line up, and a dataset holding nothing says so once rather than reporting three zeroes
that read as measurements. A layer nothing defines a dataset in is left out; a dataset nothing has
written is reported, since what is missing is half of what the question asks.

The span of a dataset with many partitions is its first and last value, which is what a reader
wants; every value is there on request. Those ends are the ends of the span because
[partition values sort in the order they were written](../../docs/medallion.md#general-rules).
An artefact has no rows, so it shows what it weighs and which versions of it exist.

Nothing here reads a row: the counts come from each parquet file's own footer, so a store far
too large to scan costs the same to summarise.
