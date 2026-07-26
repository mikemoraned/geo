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
never changes. Corrections arrive as a new version, not an edit.

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
- A clean lat/lon geometry in a global CRS (CRS 84) is always present.
- A column in the projected CRS most appropriate to the entity may additionally be
  pre-computed, since metric distance calculations must not be performed in degrees. Use
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

Per-dataset Hive partition keys are the schema of this store and are expensive to change
later, so they are pinned rather than left to each writer.

*(To be filled in — see the partitioning task in `current-slice.md`.)*
