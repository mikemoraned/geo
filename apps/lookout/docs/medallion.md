# Medallion architecture

How lookout's data is separated into layers, where it lives on disk, and what format each
layer uses. This is the [medallion
pattern](https://motherduck.com/glossary/medallion-architecture/): data flows
landing → bronze → silver → gold, getting more derived and more query-shaped at each step.

Everything up to [Options considered or deferred](#options-considered-or-deferred) describes
the store as it is. That last section holds what was weighed and set aside.

## Root

```
<root>/<layer>/<dataset>/<hive partitions>/*.parquet
```

Hive format (`key=value` directory names) throughout, so a glob queries a layer and the
partition keys come back as columns.

**The store lives in the repo, at `data/medallion`.** Bronze cannot be re-derived, so it is
versioned alongside the code that wrote it. A deletion is then recoverable from history rather
than only from a backup. Git ignores the rest of the store, but for the artefacts
[gold](#gold) names:

- **landing**, which a drain empties into bronze
- **silver and gold**, which are derived
- **bronze's upstream reference extracts**, which are large and re-derivable from the manifest
  recording what each one took

The root is found by walking up from the working directory for the manifest declaring the
workspace, as cargo does. Resolving it as a path relative to wherever a binary was started
would quietly make a second store instead of finding the one that exists. Finding no
workspace is an error saying so, not a fallback: a store in the wrong place is worse than a
run that refuses to start.

Every CLI that reads or writes the store can still be pointed elsewhere:

```
--medallion-root <PATH>
```

Every binary takes the same flag name, from one shared args definition rather than declaring
its own. A run against an external drive, or against a throwaway root in a test, is then a
single explicit argument recorded in the command.

## Layers

### landing / external

Where raw recordings are made, in whatever native live format the source uses — a queue a
device posts into, or the capture log a polling process appends to.

Formats here are optimised for **fast writes by a single writer**. A queue, an sqlite db, or a
parquet file per poll is permitted. This is the one layer where non-parquet formats are the norm.

Landing is not a durable archive: it is drained into bronze. A drain writes bronze, checks that
bronze holds every row it read, and only then deletes the landing files. Until a drain runs, a
recording exists only on the local disk.

### bronze

Raw data, extracted from landing and retained indefinitely. Three kinds:

- sensor readings captured from our own devices
- samples pulled from a third-party live service
- point-in-time extracts of an upstream reference dataset, restricted to the area and
  themes we need

**Bronze is append-only and versioned.** A write adds a file, and never changes one already
written. This is enforced rather than trusted. An append onto a path that already holds a
file fails, so two writes that would name the same file cannot silently replace each other's
rows. Two writers collide that way: a batching writer whose batches fall close together, and
an ingestion replayed twice. Batch file names therefore carry millisecond precision.

The same enforcement covers the other direction: **the layers holding observations cannot be
replaced or swept at all.** Nothing derives them, so nothing can put them back. The
operations a derived layer needs — replacing a partition, deleting the partitions a rebuild
no longer produces — therefore do not exist for them. A dataset's layer is part of its type,
so those operations are offered only where the layer permits them, and writing one against an
observation dataset does not compile. A dataset placed in a layer inherits the rule instead
of restating it, and there is no call to review or error to handle.

**A lossless transformation is the one way bronze files change.** It runs once, outside the
operations a writer has, and keeps every observation. It rewrites files in place, or copies a
version's rows into the next version and then deletes the old version's files. Git history
keeps every file it replaces, with one exception. A branch that has not merged may drop from its
own history the files of a version it both added and migrated, so the main branch never holds
them. Code at that branch's earlier commits then finds those files missing.

**Bronze tolerates the same observation arriving more than once, so deduplication is the
reader's job.** Nothing here rejects a repeat:

- an ingestion cannot rewrite an earlier file to merge into it
- a payload can reach the store by more than one route
- a re-run or a later backfill of the same source can land it again
- a drain interrupted before it deletes its landing files moves them again

Overlapping samples of a live service are duplicated deliberately. Every row therefore
carries what identifies the observation it holds — a content hash, or the natural key of the
reading. Each derivation collapses on that identity before it does anything else. A
derivation must not assume a distinct set of observations because the store currently happens
to contain one.

Format: parquet, shaped for **quick, safe appends**.

- The structure is biased towards samples rather than entities: each poll or drain is
  identified by a timestamp, and that timestamp forms part of the folder structure.
- One file per ingestion run, not per data point — an ingestion extracts many readings and
  writes them as a single batch. Small files are therefore acceptable at this layer, since
  querying is done against silver.
- **Samples merge before they reach git.** Bronze never rewrites a file, and git keeps every
  committed blob, so merging committed files frees nothing. A polling process therefore writes to
  landing, and each drain writes bronze files holding many polls.
- **A compressed file pays only when it holds many polls.** Git finds no deltas between compressed
  files, so compressing a file per poll takes more room in git than leaving it uncompressed. Sorting
  a merged file puts repeated values next to each other, where its encodings remove them.
- **A value that samples repeat may move to a dataset of its own**, stored once under a hash of its
  text and still verbatim. A sample's row then carries the hash. Writing that dataset before the
  samples means a crash leaves an unused value, and never a missing one.
- **Compact third-party geo formats are retained as received.** An encoded
  [polyline](https://developers.google.com/maps/documentation/utilities/polylinealgorithm)
  arriving from a live service is stored verbatim as a polyline. Such formats are not the
  preferred representation, as they cannot express everything recorded elsewhere, but
  re-encoding would lose fidelity to the source. Geo data generated here uses the silver
  formats instead.
- Extracts of an upstream dataset are stored **largely in that dataset's native shape**,
  plus provenance: which upstream release was used, when the extract was taken, and the
  bbox it was restricted to. Provenance lives in a separate `extract` table we own,
  holding extract id, date, release, and bbox. The upstream rows themselves only gain an
  `extract_id` column.

### silver

Cleaned, normalised, query-shaped derivations of bronze — raw readings segmented and
normalised into standard geometries, and upstream reference data enriched, extended, and
restricted to what is required.

Format: **[GeoParquet 1.1.0](https://geoparquet.org/releases/v1.1.0/)**, optimised for fast
and scalable lookup. Metadata that makes queries faster, such as bboxes, is embedded.

- A geometry column holds [WKB](https://libgeos.org/specifications/wkb/)-encoded
  [simple features](https://www.ogc.org/standards/sfa).
- A clean lat/lon geography in a global CRS (CRS 84) is always present, in a column named
  the same across every dataset. A reader therefore finds a dataset's geography without
  knowing which dataset it is.
- A column in the projected CRS most appropriate to the entity may additionally be
  pre-computed, since metric distance calculations must not be performed in degrees. It is
  likewise named the same across every dataset. Use **one projected zone per country**: a
  country's *zone* is the single projected CRS its metric geometry is written in, chosen for the
  country rather than for the strip of longitude a point happens to fall in. Several UTM strips
  can cross a country, and one zone keeps every geometry within it directly comparable; a zone
  not centred on the country costs some scale error at the far edges, which a comparison made
  within one country carries.
- **A zone is chosen to need no datum transformation.** Every engine writing the projected column
  has to reach the same numbers, and a transformation one applies where another skips it is how
  they diverge: a projection on a local datum leaves the result depending on which transformation
  grids a machine has installed, which is a property of the machine rather than of the store. A
  CRS is a projection plus a datum, and it is the datum half that differs — Ordnance Survey's [A
  Guide to Coordinate Systems in Great
  Britain](https://www.ordnancesurvey.co.uk/documents/resources/guide-coordinate-systems-great-britain.pdf)
  sets out the eight parameters a datum carries (§3.2) and why any two ellipsoids differ in size,
  shape, position and orientation (§5.2.1), and PROJ's [Geodetic
  transformation](https://proj.org/en/stable/usage/transformation.html) counts the steps the
  journey takes. A zone on the same datum as the lat/lon column skips all of it.
- CRS is recorded in the GeoParquet metadata as PROJJSON.
- **The lat/lon column holds a row's geography, and the projected column holds its geometry.** A
  geography holds angular coordinates on the round Earth, where a distance runs along a great
  circle. A geometry holds coordinates on a plane, where a distance is a straight line. [Section 4.3
  of the PostGIS manual](https://postgis.net/docs/using_postgis_dbmanagement.html#PostGIS_Geography)
  states the distinction. A projection always lands on a plane, so a projected column holds a
  geometry. A geometry need not be projected, since a PostGIS geometry can hold lat/lon.
- **A file does not state this distinction.** Both columns are simple features, so the term
  *geometry column* covers both. Neither sets the `edges` key of
  [GeoParquet](https://geoparquet.org/releases/v1.1.0/), so a reader of the file takes both as
  planar. The column named `geometry` holds the geography, and `geometry_projected` holds the
  geometry.
- **Follow the upstream schema** when extending or subsetting a reference dataset, and also
  when creating a dataset from scratch, as a mature upstream schema generally already fits
  the requirement.
- **A column that identifies a row is declared as such on the dataset, and the writer enforces
  it.** Silver is where identity is decided: bronze tolerates a repeated observation, and a
  derivation collapses on that identity to produce these rows. A name a reader can treat as
  identifying one row is therefore checked once, where the dataset is defined, rather than by
  each writer and each consumer. The check spans the dataset rather than a partition of it.
  Which partition a row lands in is a fact about how it is stored, not about what it is
  called. A dataset may carry more than one such name, each identifying a row on its own. That
  second name exists for a compact form of an id, meant for a consumer with no room for the
  first — exactly the case where a collision would otherwise surface downstream.

#### Writing silver

A derivation does not assemble a silver dataset's files itself: it hands the store its rows and
the store applies the format. Four things follow from the dataset's definition, and are applied
in one place:

- which partition a row belongs to
- the CRS its geography is projected into and declared in
- the deletion of a partition a rebuild no longer produces
- the names a definition says identify a row

A writer that projected its own geometry, or that swept only the partitions it remembered to,
would be a second implementation of the format with nothing holding it to the first.

A row's geometry is therefore derived from its geography rather than supplied alongside it. The
country whose zone it is written in is stated per row rather than per run.
Which entity a row belongs to is what decides where it is measured, so the rows making up one
entity are measured in one zone, wherever they individually fall.

A derivation that needs a row's metres itself, such as for a speed, takes them from the store's
projector. The projector returns the geography paired with its geometry and its country. A
derivation can hand over only that pair, so the store still decides the zone and each geography is
projected once.

An open file keeps its latest row group in memory, even after a flush. The object store's writer
buffers the group's last chunk as a slice of the group's whole allocation. A derivation that hands
the store all its rows at once therefore closes each partition's file as soon as that partition is
written. If a later partition then fails, the earlier ones stay replaced until the derivation
reruns. A derivation that hands over its rows in several calls tells the store which dates are
complete, since the store cannot infer that. The store closes those dates' files and refuses any
later row for them, because a new file would replace a closed one.

#### Writing silver from another language

A derivation prototyped outside the language the store's writers are written in — a notebook,
typically — writes through **one implementation of the silver format, not one per language**.
The format is a set of rules a second implementation would have to restate and then keep in
step: geometry encoding, CRS metadata, column types, partition layout, and what a rebuild does
to a partition it no longer produces.

The way in is therefore a binding over that implementation rather than a library for writing
these files. It takes a table in a language-neutral columnar form, so rows cross the boundary
without being copied through the calling language's objects. It is given a dataset name rather
than a path, and that dataset's definition supplies both the columns the table is checked
against and the layout it is written in. A table whose columns are not the dataset's, or that
names a dataset outside the layer a derivation may replace, is refused.

A table must carry the dataset's own columns, its geometry columns, and the columns the
partition values are read from. Those last are written into the path and not into the file.
Each column is converted to the type the definition states, so which engine built the table
does not change what is stored.

### gold

Outputs, not inputs: the results of evaluating a particular version of a derivation
against silver, and data prepared for live external use.

Format: determined by the consumer. Specialised formats are expected at this layer, such as
[PMTiles](https://docs.protomaps.com/pmtiles/) for live map serving. Polylines are also
permitted again. Where no specialised format fits, use
[GeoArrow 0.2](https://geoarrow.org) for fast export/import, **uncompressed**, since
compressed GeoArrow is not universally supported by consuming viewers.

Laid out by what was produced and which run produced it, rather than by a partition key:

```
<root>/gold/artifact=<name>/version=<run>/<file>
```

**Gold is derived, and so is not versioned in git — with one exception.** An artefact a build
reads is an input to something outside the store, and cannot be re-derived by whoever builds
it: a checkout has no silver to pack from. The crossings and the sessions are the cases today.
The device's firmware embeds the crossings and the browser fetches them, and a kiosk replays the
sessions. Every version packed is committed, and `apps/lookout/crossings.version` and
`apps/lookout/sessions.version` name the ones being built against.

## Reading from another language

A read from outside the store's own language goes through the same binding as a write, and names
datasets rather than paths. The table names in a query are the datasets it reads, each registered
under the name the query used, so a derivation says in SQL what it wants and the store finds the
files. A name the layer holds no dataset for is refused, with the names it could have been.

A read of a derived dataset names the dataset, and names the country too where that dataset holds
one zone per country. Reading such a dataset without one is refused rather than answered from an
arbitrary partition, and geometry comes back in the store's own encoding. A caller asks the store
which countries a dataset holds, so reading every country appends one read per country instead of
keeping a list of them in step.

A read of an observation layer names no country, since nothing there is projected per country. A
dataset's own partition value comes back as a column, so a query reads the layout rather than
parsing it out of a path. A read of an upstream extract names a theme and type rather than an
extract: one table cannot span themes whose columns differ, and a theme and type covers every
extract taken, each row carrying the id of the extract it came from, so the provenance table joins
to the rows in plain SQL. Geometry comes back as the upstream file holds it.

## What a read costs

A read costs the bytes it scans and the rows it holds, not the size of the dataset. When a read
states its columns, filter, and order in the query, three properties of the scan keep both small:

- **A scan reads only the columns the read names.** In a dataset holding a path per row, geometry
  takes most of the bytes. A read that leaves geometry out scans a small part of each file.
- **A filter skips what statistics rule out.** A filter on a partition key skips whole partitions.
  A filter on any other column skips each file, and each row group within a file, whose minimum
  and maximum rule it out. It then applies to the rows as they stream from the scan, so rejected
  rows never accumulate.
- **A sort followed by a limit holds only the leading rows.** The scan keeps the first rows as it
  goes, rather than sorting every row first.

How much a filter skips depends on how the files order its column. A column unrelated to the order
of writing varies within every file, so its statistics rule out little. Its saving then lies in the
rows the read never holds, not in the bytes it skips.

A filter or a sort applied to rows already fetched holds every row of the dataset at once.

## No table format

The layout above *is* the metadata: partitioning is directory names, schema is the files',
and a partition is replaced by rewriting it. Nothing holds partition spec, schema history,
snapshots, or statistics beside the data.

## Dataset versions

**Every change to a dataset's declaration makes a new version of the dataset.** The declaration
is what a dataset's code states about it:

- **the schema its files store**: each column's name, position, type, and nullability, with any
  nested fields and column metadata, such as a geometry column's CRS
- **the constraints its row type adds to that schema**: a checked newtype, an enum stored as
  text, a column declared unique
- **its name, layer, and partition key**, since a reader gets a partition key back as a column

A reader finds a column by name, or by position as a dataframe can, so a column's position is
part of the declaration. The rule covers an added optional column, which breaks no reader that
finds columns by name, and a widened type. A version's files therefore share one declaration. A
test records every declaration, and fails on any change to one.

A change of meaning leaves the declaration as it was. Such changes include a new unit, another
value standing for "none applies", a new grain, a row identity that varies between runs, and a new
spelling of a partition value. The declaration test cannot find one. Where a person judges one
significant, they give the dataset a new version, as for a change to its declaration.

A new partition key makes a new version like any other change to a declaration, and that version's
files are all written afresh. A dataset reads under one layout, so files laid out under another key
do not read as part of it.

A reader at an earlier commit needs no support from a later version. That commit holds its own
bronze, and its own code can rederive the silver it reads.

Every version carries its number as a suffix of the dataset's name, from the first version on, as
in `_v1` and `_v2`. A first version that predates this rule has no suffix, as `raw_sample` does,
since a new name would make a new version. Readers register a dataset under its name, suffix
included, and query it by that name. A reader written against one version reads only that version,
and moving it to the next takes an edit to its queries. Under an alias without the suffix, a reader moves to the next version
unedited, and fails once a query reaches a dropped column.

## Rederivability

Provided everything is retained in bronze, **all of silver and gold is rederivable**. The
guarantee rests on bronze keeping every observation.

Deletion from silver or gold is avoided, as rederiving is slow, but is permitted where
necessary. Deleting an observation from bronze is not allowed. A lossless transformation
deletes a file only once the observations it holds live on in the files replacing it.

## The multi-engine rule

Different jobs suit different engines, and more than one is in use at any time. Currently
that is DuckDB for notebooks and ad-hoc SQL, SedonaDB for in-process spatial work in Rust,
and georust for anything embedded in a live system.

**Any file in silver must be readable by every engine in use, with no engine-specific
handling.** Per-engine variants and per-engine read caveats are not permitted. The store is
therefore independent of any single engine. Which engine a given job uses is a local
decision, not a division in the data, and an engine can be added or dropped without
migrating silver.

## One CRS to a geometry column

**A geometry column carries one CRS wherever it appears: in a file, a result set, a dataframe or
an Arrow record batch.** A column holding two CRSs describes none of its values, since no row says
which of the two it is in, and a reader that averages, measures or indexes that column computes on
mixed units.

A country also knows the bbox it spans, rounded outward: where it is, not where it ends. It
answers a sanity check — whether a point set holds only points from countries the store supports —
while placing a point uses the areas themselves, and an extract's bbox comes from the release.

Silver and gold hold only countries the store can place: a country level names one of the
countries the store defines a zone for, and a partition under any other code is a fault to report
rather than rows to read. Bronze is free of that — it records what was observed, including an
observation from a country the store cannot yet place, which is how the data arrives before the
support for it does.

A zone is chosen per country, so the projected column of a dataset partitioned by country differs
from partition to partition. Two consequences follow:

- **A read of such a dataset takes one country at a time.** A scan of the whole dataset would put
  two zones in one column, and an engine that checks refuses rather than guesses: SedonaDB will
  not plan it. A comparison in metres holds within one country anyway, so the read scopes to the
  partition the comparison needs.
- **A read across countries carries no projected column.** Every country shares the lat/lon
  column, in CRS84, so a union reads that column — as coordinates, or as a geometry whose values
  are all in one CRS — and appends the countries one after another. Which countries those are is
  asked of the store, as the partition values the dataset holds rather than as the countries a
  caller expects. That question is a query like any other — the engine projects the value out of
  the layout — and it reads the files as ordinary parquet, so the column that differs declares
  nothing and every partition answers together.

The engines disagree: DuckDB reads a mixed-CRS scan without complaint, SedonaDB rejects it. The
rule holds either way, since the data either describes itself or it does not.

## Column conventions

A few representations are fixed across the whole store rather than chosen per dataset, so
that knowing one dataset's columns is knowing them all.

- **Instants are UTC timestamps at millisecond precision**, whatever integer form they
  travel in on the way here. A column holding a time is stored as a time, never as a number
  a reader has to be told the unit and epoch of.
- **A value drawn from a closed set of names is stored as the name**, in a string column. It
  is not stored as a code to be resolved elsewhere, and not as a union type whose reading the
  engines disagree on.

## Partitioning

Per-dataset Hive partition keys are part of the schema of this store and are expensive to
change later, so they are pinned rather than left to each writer. Each dataset is defined
once, in code, as the layer it lives in, the key it is partitioned on, and the columns it
holds. Readers and writers refer to that definition rather than repeating a name, a key, or
a struct of their own. That is what gives a reader a typed way into a dataset it did not
write. Geometry columns are the exception to that definition, as they are built as arrow
rather than derived from a row type. What follows is the reasoning behind those
definitions.

### General rules

- Partition keys are **snake_case**, and values contain no `/`, spaces, or `=`.
- Dates are UTC and formatted `YYYY-MM-DD`. A date key is named for the event it records
  (`ingested_date`, `polled_date`, `sample_date`), never a bare `date`.
- A partition key records the *coarse* value only. The full-precision timestamp is also
  written as a column inside the file, so no reader depends on parsing paths.
- Partitioning is chosen to make the common filter cheap, not to make directories tidy.
  Depth is kept shallow: two keys is the normal maximum.
- A key whose value is effectively unique per row (an id) is only used where each value
  identifies a whole write — an extract or a run — never for row-level identifiers.

**Partition keys are left for the engine to discover, and their types are not declared.** A
reader that names the dataset gets the keys back as columns. A predicate over one prunes
whole files, without the reader having to state what it will find. The engines differ in the
type they infer: one reports a date key as a date, another as a string. Neither needs help,
because a date-typed predicate against a string-typed key is coerced, still prunes, and
returns the same rows, in both directions. Declaring the types buys nothing but a second
place for the layout to be written down, and one that drifts silently when a key is renamed.

This holds because dates are formatted `YYYY-MM-DD`, whose lexical and chronological order
agree — a date key compared as a string is still ordered correctly. An id minted from an
instant, `YYYYMMDDTHHMMSSZ`, agrees the same way, so partition values sort in the order they
were written whichever of the two a dataset is keyed on. A partition key in a format without
that property cannot be relied on this way.

### Bronze

Bronze partitions on **when data was captured or ingested**, because bronze is append-only
and written one batch at a time. Every write lands in a new file, and never rewrites an
existing directory.

| dataset | partitioning | file |
| --- | --- | --- |
| sensor readings, one dataset per sensor | `ingested_date=<date>` | one per ingestion, named for its instant |
| third-party service samples | `polled_date=<date>` | one per drain, named for its instant |
| values those samples repeat | none | one per drain, named for its instant |
| upstream reference extracts | `extract_id=<id>/` then the upstream's own layout, verbatim | as produced by the extraction |
| extract manifest | none | one row per extract: id, date, upstream release, ISO 3166-1 alpha-2 country, bbox |

`extract_id` is the outermost key so that everything below it is the upstream's own
directory layout, unaltered. A query written against the upstream source therefore works
against an extract by changing only the root, and the extraction never has to rewrite paths
it does not own. An extract is identified by an id rather than a date because it is the
unit of immutability: a re-extraction is a new id, not a replacement.

Fetching an extract's rows a second time is not a re-extraction, though, and keeps the id it
was first taken under. The manifest records the release and the bbox the extraction was
restricted to, and an upstream release does not change, so the same request answers with the
same rows. That is what makes the extracts re-derivable rather than versioned. It holds only
while an extract's rows are absent. This layer has no replace, so fetching over rows already
there would double them, and an extract already present is not fetched again.

The extract manifest is small and always read whole, so it is left unpartitioned.

This is the one place the two-key depth limit does not apply, since the depth below
`extract_id` is the upstream's choice.

### Silver

Silver partitions on **what queries filter by** — overwhelmingly time for observation data,
and region for reference data.

| dataset | partitioning |
| --- | --- |
| sessions | `country=<iso3166-1 alpha-2>/start_date=<date>` |
| session samples | `country=<iso3166-1 alpha-2>/sample_date=<date>` |
| derived transit legs | `country=<iso3166-1 alpha-2>/departure_date=<date>` |
| reference-derived geo datasets | `country=<iso3166-1 alpha-2>` |
| observations matched against reference data | `<event>_date=<date>` |

Samples are partitioned by the date of the sample itself, not of its session, so a session
spanning midnight is split across two partitions. `session_id` is carried as a column and
reassembles it. A dataset recording that an observation met a piece of reference data uses the
date the two met, so the same key means the same thing as it does on the observation itself.

Observation data is partitioned by country above its date. A projected geometry column states
one CRS for the whole file and the zone is chosen per country, so rows of two countries cannot
share a file and describe their metres truthfully. The country is the one the observation is
in, looked up from the reference data's own areas, not a parameter of the run that derived it.
It follows that a dataset holding no geometry has no country level: the level exists for the
projected column's CRS, and nothing else needs it.

Silver transforms are idempotent: rerunning one over unchanged bronze input produces an
identical partition, so a partition can be rebuilt rather than incrementally merged.

A rebuild replaces the whole of what it derives. A partition that exists but the rebuild
produces no rows for is therefore deleted rather than left standing, since a partition no
derivation claims is indistinguishable, to a reader, from a current one. Deletion is bounded
by the partition key being swept, so it never reaches a directory written under another one.

That applies at every level a dataset is partitioned by, not only the innermost. Sweeping a
level requires knowing every value the run produced for it, so it is done where that is
known. A rebuild covering only part of a dataset must therefore not sweep: the part it did
not derive would read as a part that produced nothing.

Rows carry the identifiers of the bronze inputs they derive from, so lineage stays traceable
whatever a dataset does about superseded rows.

### Gold

Gold partitions on **which run or version produced the output**, and nothing is overwritten.

| dataset | partitioning |
| --- | --- |
| evaluation results | `run_date=<date>/run_id=<id>` |
| exports for live use | `artifact=<name>/version=<version>` |

A run's configuration and input dataset versions are written as columns alongside its
results, so a run is interpretable without reference to the code that produced it.

**A run is written once.** A write of a run whose id the dataset already holds, under any date, is
refused, and the rows already written stay. A run with no rows still writes its partition, so a
read finds the run.

**A coordinate written out for a consumer keeps six decimal places**, about 11 cm. Silver holds a
position as `f64`, which takes seventeen significant digits to write in full: nanometres, against a
fix accurate to metres at best. The digits past the sixth cost bytes in every row of every export.

An export in a specialised format is a **file**, not a dataset: nothing queries it, and the
format has no room for columns. It is laid out the same way and holds its file inside:
`gold/artifact=<name>/version=<version>/<file>`. The version is the instant the run started,
which is the one thing that always differs, and what it cannot carry as a column belongs in
the run's log instead. Something outside the store holding one of these cannot say which run
produced it, which is why a rerun adds a version rather than replacing one.

## Options considered or deferred

Alternatives weighed and set aside, with what would justify revisiting each. None of this
describes how the store behaves now.

**A table format.** The layer that holds partition spec, schema history, snapshots, and
statistics as metadata beside the data. It would add atomic replacement of many files at
once, schema evolution, time travel, and pruning from statistics rather than from paths. Not
adopted: it costs a metadata layer to understand and an engine support matrix to track,
against a store where a partition is one file and a rewrite is therefore already atomic.
One format, Iceberg, finds a column by an id that renames and reorders keep, and partitions on
transforms of stored columns, so those changes break none of its readers.
Revisit when partition-level atomicity or schema evolution starts to cost debugging time.
Judge a candidate first on breadth of engine support, since multi-engine readability is the
rule this store is built around, and then on write maturity against its own status reporting,
since this store would be a writer and not only a reader.

**Parquet's native
[GEOMETRY/GEOGRAPHY types](https://parquet.apache.org/docs/file-format/types/geospatial/).**
Not used, as engine support for them remains limited. Silver encodes geometry as WKB instead.
Revisit when every engine in use reads them with no engine-specific handling.

**Retaining history in silver** — superseded versions of a row, or validity intervals.
Deferred, and a per-dataset decision to settle explicitly rather than by accident. It is
needed when something downstream has to know what a dataset said at an earlier time, and
unnecessary when only the current view is ever read. Whichever is chosen, apply it
consistently across silver rather than varying it dataset by dataset.

**Classifying each change to a declaration by which readers it breaks.** A change can break a new
reader on old data, or an old reader on new data. Avro, Confluent Schema Registry, Iceberg, and
Delta Lake allow the changes that avoid a chosen direction, such as an added optional column or a
widened type. Where a version's declaration can change, its files span every declaration it has had,
and a reader meets them all. A change then has to keep every earlier declaration's files readable,
not only the last one's. Not adopted: with every change making a new version, the test needs no
rules, and no such chain forms. Revisit when new versions for changes no reader can notice cost more
than those rules would.

**A marker for a change of meaning**, a string in the declaration that a person sets to force a new
version. SQLMesh's `stamp`, Dagster's `code_version`, and Confluent Schema Registry's
`major_version` work this way: each system assigns a version itself, and acts on the marker. Not
adopted: here a person names each version, so a marker's only input is a decision already taken.
Revisit if a machine comes to assign versions.
