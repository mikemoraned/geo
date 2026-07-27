# How this store compares to Rust data engineering practice (2026-07-27)

A point-in-time review, written while the medallion refactor was in progress: how the
store we built lines up with what the Rust data ecosystem does, whether to adopt a table
format, and what exists for orchestration. **Temporary** — the durable conclusions belong
in `medallion.md` or in a task; this note is deleted at the end of the slice.

## What we have, in the ecosystem's vocabulary

`DatasetSpec` + `Query::register` is a small **catalog**: it maps a name to a location.
DataFusion's equivalent layering is
[`CatalogProvider` → `SchemaProvider` → `TableProvider`](https://docs.rs/datafusion/latest/datafusion/catalog/index.html),
where a `TableProvider` yields Arrow batches and describes itself to the planner.

Other terms worth knowing, because published material uses them for what we already do:

- rebuilding a partition is a **partition overwrite** (`INSERT OVERWRITE`)
- "bronze is immutable" is an **append-only raw zone**
- one file per ingestion is the **small files problem**, whose standard answer is periodic
  **compaction** — worth planning before bronze holds thousands of poll files
- medallion itself is a *logical* pattern, not a technology: every published description
  assumes Databricks/Fabric/Snowflake, but nothing in it requires them

## Partition columns are already inferred

Checked rather than assumed, because an earlier version of this review had it wrong:

- SedonaDB auto-discovers partition columns when the reader hasn't set them
  (`sedona-geoparquet/src/provider.rs:68-91`), gated on DataFusion's
  `listing_table_factory_infer_partitions`, which **defaults to `true`**
  (`datafusion-common-52.5.0/src/config.rs:588`).
- So partition keys are present as columns, typed `Utf8View`, and a predicate on them can
  prune whole directories — [`ListingTable`](https://docs.rs/datafusion/latest/datafusion/datasource/listing/struct.ListingTable.html)
  is what provides this.

What we don't do is *ask* for less than everything: the derivations read all of bronze by
design, and filter on data columns rather than partition keys. The pruning machinery is
there and unused.

## Should the catalog traits be implemented directly?

Mostly no, because they sit at a different layer rather than competing with `DatasetSpec`.

- **`TableProvider`: no.** `ListingTable` already is one, and it is what does file listing,
  partition inference and pruning. Implementing our own means re-implementing that. The
  reason to write one is a source that isn't files — an API, a live buffer, a database.
- **`SchemaProvider` / `CatalogProvider`: later, if ever.** It would buy
  `SELECT … FROM bronze.gps_reading` without an explicit registration, and engine-side
  discoverability. Modest, against two costs:
  - **it serves one engine.** Silver must be readable by DuckDB and georust too; a spec is
    plain data all three can use, a catalog is DataFusion's view of it. The spec stays the
    truth; a catalog would be an adapter over it.
  - **version coupling.** These traits move between DataFusion releases, and the version is
    pinned by SedonaDB. Inert data survives an engine upgrade; trait impls don't.
- **Writes stay ours regardless.** `TableProvider::insert_into` would not emit GeoParquet's
  `geo` file metadata, which needs the `geoparquet` encoder.

## Table formats

Delta Lake and Iceberg exist to hold in metadata what we encode in directory names:
partition spec, schema, snapshots, statistics. Both have Rust implementations that plug
into DataFusion — [delta-rs](https://github.com/delta-io/delta-rs) and
[iceberg-rust](https://github.com/apache/iceberg-rust) (core plus REST/Glue/SQL catalogs
and an `iceberg-datafusion` crate).

They would give us atomic multi-file partition replacement, schema evolution, time travel
and stats-based pruning; they cost a metadata layer to understand and an engine support
matrix to track.

**Not yet, but know the exit.** If we go, Iceberg looks the better fit: 2026 commentary is
that [Iceberg has the edge for multi-engine, vendor-neutral setups](https://bigdataboutique.com/blog/apache-iceberg-vs-delta-lake-choosing-the-right-table-format)
while non-JVM Delta leans on community-maintained delta-rs — and multi-engine readability
is our own stated rule. Verify write maturity on [the Iceberg status page](https://iceberg.apache.org/status/)
before committing, since we would be a writer. The trigger to reconsider: when
partition-level atomicity or schema evolution starts costing debugging time.

## Orchestration

There is no Rust Airflow. The options divide in two:

**In-process DAG crates** — [`dagrs`](https://github.com/dagrs-dev/dagrs) (flow-based,
parallel task execution), [`apalis`](https://lib.rs/crates/apalis) with `apalis-workflow`
(job queue, sequential/DAG/conditional flows, retries, metrics, Postgres/Redis backends),
plus smaller ones (`dag-scheduler`, `async_dag`). These order work inside one process. None
provides scheduled runs, backfills over historical partitions, run history, lineage or
alerting — which is what an orchestrator is actually for.

**Orchestrators that run Rust as a step:**

- [**Dagster** + Pipes](https://docs.dagster.io/guides/build/external-pipelines) — Python
  control plane, but Pipes is a language-agnostic protocol, so a Rust binary runs as a
  first-class *asset* reporting logs and metadata back. Closest conceptual fit: its model is
  datasets-with-dependencies, not tasks-in-a-DAG, and backfilling a date range is native.
- [**Windmill**](https://www.windmill.dev/changelog/rust-support) — Rust core engine, added
  first-class Rust script support in Feb 2026, workers pull jobs from Postgres, scheduling
  built in. Attractive if the orchestrator itself should be Rust.
- [**Kestra**](https://kestra.io/resources/data/airflow-alternatives) — YAML orchestration
  separate from the execution layer, runs Rust among many languages. Heavier, JVM.

**Recommendation: none yet.** Three CLIs run by hand are below the threshold where an
orchestrator pays for itself, and adopting one now would freeze the pipeline's shape while
it is still moving. Signals that change the answer, in order:

1. needing to re-derive a *range* of past partitions after a bug — backfill is the feature
   that can't be cheaply hand-rolled
2. more than one consumer depending on a dataset's freshness
3. wanting a run to fail loudly when nobody is watching

At that point: Dagster Pipes for the asset model, Windmill if a Python control plane is
unwelcome. The repo's "keep Python minimal" rule is about data logic; an orchestrator is
infrastructure.

## Recommendations, in order of value

1. **Give the CLIs a date-range argument**, so a run can ask for less than everything. This
   is what makes the existing partition pruning worth having, and it is the prerequisite
   for handing the work to any orchestrator later.
2. **Decide whether partition columns should be declared with their types** rather than
   inferred as `Utf8View` (tracked as a task in `current-slice.md`).
3. **Leave the catalog traits** until registration boilerplate is genuinely annoying, then
   add a `SchemaProvider` over the dataset definitions rather than replacing them.
4. **Watch bronze file counts**, and write down a compaction plan before the small-file
   problem is real rather than after.
