# Medallion architecture

How lookout's data is separated into layers, where it lives on disk, and what format each
layer uses. This is the [medallion
pattern](https://motherduck.com/glossary/medallion-architecture/): data flows
landing → bronze → silver → gold, getting more derived and more query-shaped at each step.

## Root

```
~/Data/geo/lookout/medallion/<layer>/<dataset>/<hive partitions>/*.parquet
```

Everything is stored in Hive format (`key=value` directory names), so a layer can be
queried with a glob and the partition keys come back as columns.

If this outgrows the internal disk it moves to
`/Volumes/PRO-G40/Data/geo/lookout/medallion` — so **nothing hard-codes the root**.

Every CLI that reads or writes the store takes it as a common argument:

```
--medallion-root <PATH>   [default: ~/Data/geo/lookout/medallion]
```

The same flag name and default are used everywhere, from a shared clap args struct rather
than each binary declaring its own. A run against the external drive, or against a
throwaway root in a test, is then a single explicit argument recorded in the command.

## Layers

### landing / external

Where raw recordings are made, in whatever native live format the source uses — a queue a
device posts into, or the capture log a polling process appends to.

Formats here are optimised for **fast in-place update by a single writer**; a queue or an
sqlite db is permitted. This is the one layer where non-parquet formats are the norm.

Landing is not a durable archive: it is drained into bronze.

### bronze

Raw data, extracted from landing and retained indefinitely. Three kinds:

- sensor readings captured from our own devices
- samples pulled from a third-party live service
- point-in-time extracts of an upstream reference dataset, restricted to the area and
  themes we need

**Bronze is immutable and versioned, not merely append-only.** A given file, once written,
never changes. Corrections arrive as a new version, not an edit. This is enforced rather
than trusted: an append onto a path that already holds a file fails, so two writes that
would name the same file — a batching writer whose batches fall close together, an
ingestion replayed twice — cannot silently replace each other's rows. Batch file names
therefore carry millisecond precision.

**Bronze tolerates the same observation arriving more than once, so deduping is the reader's
job.** Nothing here rejects a repeat: an ingestion cannot rewrite an earlier file to merge
into it, a payload may reach the store by more than one route, and a re-run or a later
backfill of the same source can land it again. Overlapping samples of a live service are
duplicated deliberately. Every row therefore carries what identifies the observation it
holds — a content hash, or the natural key of the reading — and each derivation collapses on
that identity before it does anything else. A derivation must not assume a distinct set of
observations because the store currently happens to contain one.

Format: parquet, shaped for **quick, safe appends**.

- The structure is biased towards samples rather than entities: each poll or drain is
  identified by a timestamp, and that timestamp forms part of the folder structure.
- One file per ingestion run, not per data point — an ingestion extracts many readings and
  writes them as a single batch. Small files are therefore acceptable at this layer, since
  querying is done against silver.
- **Compact third-party geo formats are retained as received.** An encoded
  [polyline](https://developers.google.com/maps/documentation/utilities/polylinealgorithm)
  arriving from a live service is stored verbatim as a polyline. Such formats are not the
  preferred representation, as they cannot express everything recorded elsewhere, but
  re-encoding would lose fidelity to the source. Geo data generated here uses the silver
  formats instead.
- Extracts of an upstream dataset are stored **largely in that dataset's native shape**,
  plus provenance: which upstream release was used, when the extract was taken, and the
  bounding box it was restricted to. Provenance lives in a separate `extract` table we own
  (extract id, date, release, bbox); the upstream rows themselves only gain an
  `extract_id` column.

### silver

Cleaned, normalised, query-shaped derivations of bronze — raw readings segmented and
normalised into standard geometries, and upstream reference data enriched, extended and
restricted to what is required.

Format: **[GeoParquet 1.1.0](https://geoparquet.org/releases/v1.1.0/)**, optimised for fast
and scalable lookup — embed whatever metadata makes queries faster e.g. bounding boxes.

- Geometry is [WKB](https://libgeos.org/specifications/wkb/)-encoded
  [simple features](https://www.ogc.org/standards/sfa). Parquet's native
  [GEOMETRY/GEOGRAPHY types](https://parquet.apache.org/docs/file-format/types/geospatial/)
  are not used, as engine support for them remains limited.
- A clean lat/lon geometry in a global CRS (CRS 84) is always present, in a column named
  the same across every dataset, so a reader finds a dataset's geometry without knowing
  which dataset it is.
- A column in the projected CRS most appropriate to the entity may additionally be
  pre-computed, since metric distance calculations must not be performed in degrees. It is
  likewise named the same across every dataset. Use
  **one projected zone per country**: several UTM zones may cover a country, but a single
  zone keeps every geometry within that country directly comparable.
- CRS is recorded in the GeoParquet metadata as PROJJSON.
- **Follow the upstream schema** when extending or subsetting a reference dataset, and also
  when creating a dataset from scratch, as a mature upstream schema generally already fits
  the requirement.

### gold

Outputs, not inputs: the results of evaluating a particular version of a derivation
against silver, and data prepared for live external use.

Format: determined by the consumer. Specialised formats are expected at this layer, such as
[PMTiles](https://docs.protomaps.com/pmtiles/) for live map serving; polylines are also
permitted again. Where no specialised format fits, use
[GeoArrow 0.2](https://geoarrow.org) for fast export/import, **uncompressed**, since
compressed GeoArrow is not universally supported by consuming viewers.

## No table format

The layout above *is* the metadata: partitioning is directory names, schema is the files',
and a partition is replaced by rewriting it. A table format — the layer that holds partition
spec, schema history, snapshots and statistics as metadata beside the data — would add atomic
replacement of many files at once, schema evolution, time travel, and pruning from statistics
rather than from paths.

**Deliberately not adopted.** It costs a metadata layer to understand and an engine support
matrix to track, against a store small enough that a partition is one file and a rewrite is
therefore already atomic. The trigger to reconsider is partition-level atomicity or schema
evolution starting to cost debugging time. Whichever is chosen then should be judged first on
breadth of engine support, since multi-engine readability is the rule this store is built
around; write maturity needs verifying against the implementation's own status reporting,
because this store would be a writer and not only a reader.

## Rederivability

Provided everything is retained in bronze, **all of silver and gold is rederivable**.
Bronze immutability is what this guarantee rests on.

Deletion from silver or gold is avoided, as rederiving is slow, but is permitted where
necessary. Deleting from bronze is not allowed.

## The multi-engine rule

Different jobs suit different engines, and more than one is in use at any time — currently
DuckDB for notebooks and ad-hoc SQL, SedonaDB for in-process spatial work in Rust, and
georust for anything embedded in a live system.

**Any file in silver must be readable by every engine in use, with no engine-specific
handling.** Per-engine variants and per-engine read caveats are not permitted. The store is
therefore independent of any single engine: which engine a given job uses is a local
decision, not a division in the data, and an engine can be added or dropped without
migrating silver.

## Partitioning

Per-dataset Hive partition keys are part of the schema of this store and are expensive to
change later, so they are pinned rather than left to each writer. Each dataset is defined
once, in code, as the layer it lives in, the key it is partitioned on and the columns it
holds; readers and writers refer to that definition rather than repeating a name, a key or
a struct of their own — which is what gives a reader a typed way into a dataset it did not
write. Geometry columns are the exception to that definition, as they are built as arrow
rather than derived from a row type. What follows is the reasoning behind those
definitions.

### General rules

- Partition keys are **snake_case**, and values contain no `/`, spaces or `=`.
- Dates are UTC and formatted `YYYY-MM-DD`. A date key is named for the event it records
  (`ingested_date`, `polled_date`, `fix_date`), never a bare `date`.
- A partition key records the *coarse* value only. The full-precision timestamp is also
  written as a column inside the file, so no reader depends on parsing paths.
- Partitioning is chosen to make the common filter cheap, not to make directories tidy.
  Depth is kept shallow: two keys is the normal maximum.
- A key whose value is effectively unique per row (an id) is only used where each value
  identifies a whole write — an extract or a run — never for row-level identifiers.

**Partition keys are left for the engine to discover, and their types are not declared.** A
reader that names the dataset gets the keys back as columns and a predicate over one prunes
whole files, without the reader having to state what it will find. The engines differ in the
type they infer — one reports a date key as a date, another as a string — and neither needs
help: a date-typed predicate against a string-typed key is coerced, still prunes, and returns
the same rows, in both directions. Declaring the types buys nothing but a second place for
the layout to be written down, and one that drifts silently when a key is renamed.

This holds because dates are formatted `YYYY-MM-DD`, whose lexical and chronological order
agree — a date key compared as a string is still ordered correctly. A partition key in a
format without that property could not be relied on this way.

### Bronze

Bronze partitions on **when data was captured or ingested**, because bronze is immutable
and written one batch at a time: every write lands in a new file, and never rewrites an
existing directory.

| dataset | partitioning | file |
| --- | --- | --- |
| sensor readings, one dataset per sensor | `ingested_date=<date>` | one per ingestion, named for its instant |
| third-party service samples | `polled_date=<date>` | one per poll, named for its poll timestamp |
| upstream reference extracts | `extract_id=<id>/` then the upstream's own layout, verbatim | as produced by the extraction |
| extract manifest | none | one row per extract: id, date, upstream release, bbox |

`extract_id` is the outermost key so that everything below it is the upstream's own
directory layout, unaltered. A query written against the upstream source therefore works
against an extract by changing only the root, and the extraction never has to rewrite paths
it does not own. An extract is identified by an id rather than a date because it is the
unit of immutability: a re-extraction is a new id, not a replacement.

The extract manifest is small and always read whole, so it is left unpartitioned.

This is the one place the two-key depth limit does not apply, since the depth below
`extract_id` is the upstream's choice.

### Silver

Silver partitions on **what queries filter by** — overwhelmingly time for observation data,
and region for reference data.

| dataset | partitioning |
| --- | --- |
| sessions | `start_date=<date>` |
| session fixes | `fix_date=<date>` |
| derived transit legs | `departure_date=<date>` |
| reference-derived geo datasets | `country=<iso3166-1 alpha-2>` |

Fixes are partitioned by the date of the fix itself, not of its session, so a session
spanning midnight is split across two partitions; `session_id` is carried as a column and
reassembles it.

Silver transforms are idempotent: rerunning one over unchanged bronze input produces an
identical partition, so a partition can be rebuilt rather than incrementally merged.

Whether a silver dataset additionally **retains history** — superseded versions of a row, or
validity intervals — is a per-dataset decision, and one that should be settled explicitly
rather than by accident. It is needed when something downstream has to know what the
dataset said at an earlier time, and unnecessary when only the current view is ever read.
Whichever is chosen, apply it consistently across silver rather than varying it
dataset-by-dataset. Rows carry the identifiers of the bronze inputs they derive from, so
lineage remains traceable either way.

### Gold

Gold partitions on **which run or version produced the output**, and nothing is overwritten.

| dataset | partitioning |
| --- | --- |
| evaluation results | `run_date=<date>/run_id=<id>` |
| exports for live use | `artifact=<name>/version=<version>` |

A run's configuration and input dataset versions are written as columns alongside its
results, so a run is interpretable without reference to the code that produced it.
