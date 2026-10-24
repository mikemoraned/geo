# What makes a change to a dataset breaking

*Research of 2026-10-24. It draws on how six systems classify a change to a schema: the Avro
specification, Confluent Schema Registry, Apache Iceberg, Delta Lake, dbt model contracts, and buf.
It applies them to the store [medallion.md](medallion.md) describes, and to the row types that
declare its datasets. Whatever holds of it belongs in `medallion.md`, and the rest goes stale.*

Which changes to a dataset break its readers, and so make a new version of it. It is for whoever
writes the rules into `medallion.md`, and for whoever builds a test that a dataset's declaration
still matches its version.

A reader is any code that reads a dataset, such as a row type, a query, a notebook, or a binding.
A dataset's declaration is what its code states about it: the schema its files store, the
constraints its row type adds, and its spec. A constraint restricts a column's values beyond the
type its files store, as an enum stored as text does.

## Two directions of compatibility

A dataset's declaration changes over time. Each change either keeps the dataset's version, the
name it is read by, or starts a new version under a new name. Compatibility concerns the
successive declarations under one name.

- **Backward compatible**: a reader written for the new declaration reads data written under an
  earlier one.
- **Forward compatible**: a reader written for an earlier declaration reads data written under the
  new one.

Confluent Schema Registry checks either direction, or both at once as full compatibility. Each
check compares a new schema with the last schema registered, or, in its transitive form, with every
schema registered before it. The two forms can disagree, since compatibility at each step does not
add up to compatibility end to end. Take three successive declarations of one dataset:

1. **A** has no `label` column.
2. **B** adds `label` as an optional column. A reader written for B reads A's files with `label`
   as null, so B is backward compatible with A.
3. **C** makes `label` required. Every row written under B holds a label, so a reader written for
   C reads B's files, and C is backward compatible with B.

A reader written for C fails on A's files, which have no `label` column. The check against the
last declaration passes C, and the transitive check fails it.

A reader meets every file a dataset holds, so the check that matters covers the declarations those
files were written under. Bronze appends by default, so a dataset keeps files written under every
declaration its version has had. Backward compatibility there has to hold transitively. A lossless
transformation can still replace bronze files, in place or by migrating a version's rows into the
next version. Git history keeps the files replaced. Where each missing label can be derived, a
lossless migration rewrites A's and B's files under C's declaration. Only C's declaration then
remains in the dataset, and C passes.

A derivation replaces a silver dataset whole, so its files share one declaration. A reader meets an
older declaration only where its code has moved on and the derivation has not run since. It meets a
newer one only where its code is older than the derivation's, such as an earlier commit checked out
beside today's silver.

## Changes to the set of columns

- **Adding a column** breaks no reader that finds columns by name. Avro fills a column a reader
  expects from the reader's default, and fails where the reader declares none. Confluent's
  backward compatibility therefore allows only an optional column to be added. dbt counts an
  added column as non-breaking.
- **Removing a column** breaks every reader of it. dbt counts it as breaking. Confluent's backward
  compatibility allows it, since a reader of the new declaration no longer asks for the column. Its
  forward compatibility allows it only for an optional column.
- **Renaming a column** removes one column and adds another, for a reader that finds columns by
  name. dbt counts it as breaking. Iceberg finds a column by an id that a rename keeps, so a
  rename breaks nothing there. Avro matches a renamed column through an alias.
- **Reordering columns** breaks no reader that finds columns by name or id, and Iceberg and Avro
  both allow it. It breaks a reader that finds columns by position. A dataframe is one: a reader
  can access its columns by index, so their order is part of its type. A query using `SELECT *`
  to feed an `INSERT` or a `UNION` is another.

## Changes to a column's type

- **Widening a type** keeps every stored value representable. Iceberg widens an `int` to a `long`,
  a `float` to a `double`, and a decimal to a greater precision. Avro promotes an `int` to a
  `long`, `float`, or `double`, a `long` to a `float` or `double`, and a `float` to a `double`.
  Delta Lake widens the same kinds of type. dbt counts any change to a column's type as breaking.
- **Narrowing a type** breaks a reader on every stored value the new type cannot hold.

An Arrow type says more than the kind of value. A timestamp carries its unit and time zone, and a
decimal its precision and scale. Column metadata carries a geometry's extension type and CRS. A
row type reads each column into the Rust type it declares, so a change of type reaches every row
type reading that column.

## Changes to nullability and constraints

Tightening a column breaks backward compatibility, and loosening it breaks forward compatibility.

- **Making a column required** tightens it. A reader of the new declaration fails on an old row
  holding a null. Iceberg counts the change as incompatible.
- **Adding a constraint** tightens a column too. In the store, an enum stored as text is one, and
  a checked newtype, such as an id that refuses an empty string, is another. Delta Lake refuses a
  new `CHECK` constraint that any stored row breaks. A constraint every stored row meets breaks no
  reader of the stored data.
- **Making a column optional** loosens it, and Iceberg allows the change. A reader written for an
  earlier declaration fails on the first null. For a row type, that is any field not declared as an
  `Option`.
- **Removing or loosening a constraint** breaks a reader that relied on it. dbt counts removing or
  modifying a column's constraint as breaking. An enum gaining a variant is one such change: Avro
  fails on a symbol the reader's enum lacks, unless that enum declares a default.

## Changes to the partition key

The store keeps a partition key's value in the path, not the file, and a reader gets the key back
as a column. The plain parquet read that registers bronze declares each key as text. DuckDB infers
a type from the values instead. A reader also finds a dataset's files through its partition keys.
Files laid out under another key do not read as part of the dataset.

A change to the partition key therefore changes the columns a reader sees, and the rules for
columns apply:

- **Renaming a key** removes one column and adds another.
- **Dropping a partition level** removes a column, unless its values move into the files.
- **Moving a column between the files and the path** keeps its name, but its type then comes from
  the reader rather than the files. The store's bronze read gives text, so a date the files stored
  as a date comes back as text. The move also reorders the columns, since DataFusion and DuckDB
  both put partition columns after a file's own.

Every such change leaves the existing files under the old layout. A dataset reads under one layout,
so a change of partition key rewrites every file in place, or starts a new version.

Iceberg hides partitioning from readers. It partitions on transforms of stored columns, such as the
day of a timestamp, and records each file's partitioning in the table's metadata. A reader filters
on stored columns alone, so a change of partitioning breaks no reader, and old files keep their old
layout. The store has no table format, so its partitioning is part of what a reader relies on.

## Changes no declaration shows

Some changes keep every column's name, type, and constraints, and still break a reader:

- **Meaning**: a column's unit, where its name does not carry it, or the value standing for "none
  applies", such as 0 in place of a null.
- **Identity**: which columns identify a row, and whether a derivation gives each row the same
  identity on every run. On 24 October, a re-derivation of `water_crossing_v2` renamed a crossing
  that two rows of `session_crossing` named.
- **Grain**: what one row stands for. `session_crossing` moved from one row per session and
  crossing to one row per pass.
- **Layout**: how a partition value is spelt in a path, such as the format of a date.

dbt counts a fix to a column's calculation as non-breaking, though a reader sees different values.

## The store's declaration

A reader of the store relies on three parts of a dataset's declaration:

1. **The schema its files store**: its columns' names, order, types, and nullability, their
   nested fields, and their metadata. The row type's serde trace gives every column but the
   geometry, and the writer adds the geometry columns with their extension type and CRS. A
   partition column goes into the path, not the file.
2. **The constraints its row type adds**: each checked newtype, each enum stored as text, and the
   columns the row type declares unique.
3. **Its spec**: its name with its version, its layer, and its partition key.

A hash of the stored schema covers the first part alone. An enum stored as text has the same Arrow
type whatever its variants. A checked newtype has the type of the `String` it checks. A hash
covering the second part needs each column's Rust type, each enum's variants, and the unique
columns. A change inside a newtype's check leaves the newtype's name as it was, so no hash of
names finds it.

## Testing a declaration against a stored one

Three systems compare a schema with a stored copy, as a test of the store's declarations would:

- Confluent Schema Registry checks each new schema against the schemas registered before it, in
  the direction a subject is set to.
- dbt compares each versioned model's contract with its previous state, and fails on a breaking
  change. A breaking change then needs a new model version.
- `buf breaking` compares Protobuf files with an earlier copy, such as one on a git branch, at four
  levels of strictness.

A hash shows that a declaration changed, not how. A test that stores each declaration as text
shows the change in its failure, as a snapshot test does. A short hash can still come from that
text.

## Open questions

- Whether any change to a declaration makes a new version, or only a change these rules count as
  breaking. Under the first, an added optional column makes a version too.
- How a test finds a change inside a checked newtype's check.
- How a change no declaration shows, such as a column's meaning, gets recorded.

## References

- [Apache Avro 1.9.2 specification](https://avro.apache.org/docs/1.9.2/spec.html), "Schema
  Resolution"
- [Confluent Schema Registry, "Schema Evolution and Compatibility"](https://docs.confluent.io/platform/7.2/schema-registry/avro.html)
- [Apache Iceberg, "Evolution"](https://iceberg.apache.org/docs/latest/evolution/)
- [Delta Lake, "Constraints and Checks"](https://delta.io/blog/2022-11-21-delta-lake-contraints-check),
  and [Databricks, "Type widening"](https://docs.databricks.com/aws/en/ingestion/cloud-object-storage/auto-loader/type-widening)
- [dbt, "Model contracts"](https://docs.getdbt.com/docs/mesh/govern/model-contracts), and
  ["Model versions"](https://docs.getdbt.com/docs/mesh/govern/model-versions)
- [buf, "Detecting breaking changes"](https://buf.build/docs/breaking/)
