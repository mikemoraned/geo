# What makes a change to a dataset breaking

*Research of 2026-10-24. It draws on how six systems classify a change to a schema: the Avro
specification, Confluent Schema Registry, Apache Iceberg, Delta Lake, dbt model contracts, and buf.
Google's AIP-180 adds how a change of meaning is judged, and Rust crates how a check is
fingerprinted. It applies them to the store [medallion.md](medallion.md) describes, and to the row
types that declare its datasets. Whatever holds of it belongs in `medallion.md`, and the rest goes
stale.*

Which changes to a dataset break its readers, and so make a new version of it. It is for whoever
writes the rules into `medallion.md`, and for whoever builds a test that a dataset's declaration
still matches its version.

A reader is any code that reads a dataset, such as a row type, a query, a notebook, or a binding.
A dataset's version is the name a reader reads it by, and a breaking change starts a new one. A
declaration is what a dataset's code states about it. A constraint restricts a column's values
beyond the type its files store.

## What a reader relies on

A dataset's declaration has three parts:

1. **The schema its files store.** Each column has a name, a position, an Arrow type, and a
   nullability, and a struct column has nested fields. An Arrow type says more than the kind of
   value: a timestamp carries its unit and time zone, and a decimal its precision and scale. Column
   metadata carries a geometry's extension type and CRS. The row type's serde trace gives every
   column but the geometry, and the writer adds the geometry columns.
2. **The constraints its row type adds.** A checked newtype, such as an id that refuses an empty
   string, restricts the text column it reads. So does an enum stored as text. The row type also
   declares which columns hold unique values.
3. **Its spec**: its name with its version, its layer, and its partition key.

A reader finds a column by name, as a query or a row type does, or by position, as a dataframe can.
The order of a dataframe's columns is part of its type.

A partition key's value lives in the path, not the file, and a reader gets it back as a column,
after the file's own. Its type comes from the reader. The plain parquet read that registers bronze
declares each key as text, and DuckDB infers a type from the values. A reader also finds a
dataset's files through its partition keys. Files laid out under another key do not read as part of
the dataset.

A reader also relies on what no declaration states:

- **Meaning**: a column's unit, where its name does not carry it, or the value standing for "none
  applies", such as 0 in place of a null.
- **Stable identity**: whether a derivation gives each row the same identity on every run. On 24
  October, a re-derivation of `water_crossing_v2` renamed a crossing that two rows of
  `session_crossing` named.
- **Grain**: what one row stands for. `session_crossing` moved from one row per session and
  crossing to one row per pass.
- **The spelling of a partition value** in a path, such as the format of a date.

## Two directions

A change to a declaration can break a reader in two ways: a new reader can fail on old data, or an
old reader on new data. Confluent Schema Registry calls a change that avoids the first backward
compatible, and one that avoids the second forward compatible. A change usually called
"backwards-incompatible", such as dropping a column, is of the second kind.

### A new reader, old data

Bronze appends by default. Where a version's declaration can change, a dataset then holds files
written under each declaration the version has had. A reader reads them all at once, so a change
has to keep every earlier declaration's files readable, not only the last one's. Confluent checks
a new schema against the last one alone, or, in its transitive form, against every earlier one.
The two can disagree. Take three successive declarations of one dataset:

1. **A** has no `label` column.
2. **B** adds `label` as an optional column. A reader written for B reads A's files with `label` as
   null.
3. **C** makes `label` required. Every row written under B holds a label, so a reader written for C
   reads B's files.

Each change is backward compatible with the one before it. A reader written for C still fails on
A's files, which have no `label` column.

A lossless rewrite ends the chain. Bronze files can be rewritten in place, or migrated into the next
version, and git history keeps the files replaced. Where each missing label can be derived, a
rewrite of A's and B's files under C's declaration leaves only C's in the dataset.

A derivation rewrites a silver dataset whole, under one declaration. A new reader meets old silver
only between a change and the next derivation.

### An old reader, new data

An old reader is one written for an earlier declaration. In the store, it is a reader the change did
not update, such as a notebook or a query in another crate. A breaking change gives the dataset a
new name, so every reader takes an edit before it reads the new data.

A reader at an earlier commit needs no support. That commit holds its own bronze, and its own code
can re-derive the silver it reads.

## Which changes break a reader

For a reader that finds columns by name:

| Change | New reader, old data | Old reader, new data |
| --- | --- | --- |
| Add an optional column | safe | safe |
| Add a required column | breaks | safe |
| Remove a column | safe | breaks |
| Rename a column | breaks | breaks |
| Widen a column's type | safe | breaks |
| Narrow a column's type | breaks | safe |
| Make a column optional | safe | breaks |
| Make a column required | breaks | safe |
| Add or tighten a constraint | breaks, unless every stored row meets it | safe |
| Remove or loosen a constraint | safe | breaks |

A reader that finds columns by position also breaks on any change to their sequence: a column
added, removed, or reordered. An old reader still reads new data where the change adds a column
after every other. In a partitioned dataset, a column added to the files never comes last, since a
reader puts the partition columns after the file's own.

### Columns

Confluent's backward check allows removing a column and adding an optional one. Its forward check
allows adding a column and removing an optional one. Avro lets an optional column go because a
reader fills a missing column from the default it declares. A query naming a missing column has no
default to fall back on. dbt counts removing, renaming, or retyping a column as breaking, and adding
one as safe.

### Types

Iceberg widens an `int` to a `long`, a `float` to a `double`, and a decimal to a greater precision.
Avro promotes an `int` to a `long`, `float`, or `double`, a `long` to a `float` or `double`, and a
`float` to a `double`. Delta Lake widens the same kinds of type. Each lets a reader's type be wider
than the writer's, which is safe for a new reader of old data. A row type reads each column into the
Rust type it declares, so a change of type reaches every row type reading that column.

### Nullability and constraints

Tightening a column breaks a new reader on old data, and loosening it breaks an old reader on new
data. Iceberg allows making a column optional, and counts making one required as incompatible. Delta
Lake refuses a new `CHECK` constraint that any stored row breaks, so a constraint it accepts breaks
no reader of the stored data. dbt counts removing or modifying a column's constraint as breaking. An
enum gaining a variant loosens its column: Avro fails on a symbol the reader's enum lacks, unless
that enum declares a default.

### Partition keys

A partition key reads as a column, so the rules for columns apply to it:

- **Renaming a key** renames a column.
- **Dropping a partition level** removes a column, unless its values move into the files.
- **Moving a column between the files and the path** keeps its name but changes where its type
  comes from. A file states its columns' types, and a reader chooses a partition key's. The column
  also changes position, since a reader puts the partition columns last.

Every such change also leaves the existing files under the old layout. A dataset reads under one
layout, so a change of partition key rewrites every file in place, or starts a new version.

### What a table format changes

Iceberg finds a column by an id that renames and reorders keep. It partitions on transforms of
stored columns, such as the day of a timestamp, and records each file's partitioning in its
metadata. Renaming, reordering, and repartitioning then break none of its readers. Avro matches a
renamed column through an alias. The store has no table format: a reader finds a column by name or
position, and the partitioning is the directory names.

### Changes no declaration shows

A change to what no declaration states, such as a column's meaning, leaves the declaration as it
was, and can still break a reader. No system finds such a change for itself. Each leaves the
judgement to a person:

- Google's AIP-180 sets semantic compatibility beside source and wire compatibility. Code written
  against an earlier version has to keep receiving what a reasonable developer would expect, and
  deciding that is a judgement call.
- dbt names changes that can break consumers, depending on the business logic: the logic behind a
  metric, the grain, a filter. Their author decides whether one needs a new model version. A fix to
  a column's calculation needs none.
- Confluent Schema Registry lets a person declare a break, whatever its cause. A metadata property,
  such as `major_version`, splits a subject's history into compatibility groups, and the registry
  checks a new schema only within its group. A schema registered with a new value starts a new
  group.

## The store's choice

Every change to a declaration counts as breaking, and makes a new version. That includes an added
optional column, which breaks no reader that finds columns by name. It also includes a widened type,
which Iceberg, Avro, and Delta Lake allow. The choice, made on 24 October, keeps the test of a
declaration simple. Two things follow:

- Each version's files share one declaration, so no chain of declarations forms, and no check needs
  its transitive form.
- A test needs no rules to classify a change: any difference in a declaration means a new version.

A change of meaning stays a judgement call, as AIP-180 and dbt treat it. Where a person judges one
significant, they set a semantics tag in the declaration. The tag is empty by default, and a hash of
the declaration covers it. Setting it therefore forces a new version, as a new `major_version` does
in Confluent's registry.

## Testing a declaration against a stored one

### What the hash covers

A hash of the schema the files store covers the first part of a declaration alone. An enum stored
as text has the same Arrow type whatever its variants, and a checked newtype has the type of the
`String` it checks. A hash of the whole declaration also needs each column's Rust type, each enum's
variants, the unique columns, and the spec. A change inside a newtype's check leaves the newtype's
name as it was, so no hash of names finds it.

### Fingerprinting a check

Rust crates offer four ways to fingerprint a type and its checks:

- **The type's structure.** serde-reflection traces the formats serde sees into a registry: each
  struct's fields, each enum's variants, each newtype's name. type_hash derives a hash of a type's
  definition, meant for detecting a message type that differs between versions. Neither sees what
  a check does.
- **The check's tokens.** hashfn is an attribute on a function. It adds a constant holding a hash
  of the function's tokens, such as `NEW_HASH` beside `fn new`. A declaration's hash can fold in
  the constant of each checked newtype it uses. The hash sees one function's tokens, not the
  functions it calls, and any edit to its code changes it, a refactor included.
- **A check declared as data.** nutype states a newtype's checks in its attribute, such as
  `validate(not_empty)`, and refuses a value that fails them, deserialisation included. A hash of
  the attribute's tokens then covers the check. schemars carries the constraints of garde and
  validator attributes into the JSON Schema it generates, but not nutype's. facet records each
  type's attributes in a `SHAPE` constant, and its validators, such as `min_length`, run as it
  deserialises. Its validators run only when facet deserialises, and the store reads rows through
  serde.
- **The check's behaviour.** A fixed list of inputs, run through each check, gives a table of
  accepted and refused values. A snapshot of that table changes with the behaviour alone, and only
  for the inputs on the list.

Under the store's choice, a token hash makes any edit to a shared check a new version of every
dataset using it. A behavioural snapshot makes one only where what the check accepts changes.

### Comparing with a stored copy

Three systems compare a schema with a stored copy:

- Confluent Schema Registry checks each new schema against those registered before it, in the
  direction a subject is set to.
- dbt compares each versioned model's contract with its previous state, and fails on a breaking
  change, which then needs a new model version.
- `buf breaking` compares Protobuf files with an earlier copy, such as one on a git branch, at four
  levels of strictness.

A hash shows that a declaration changed, not how. A test that stores each declaration as text shows
the change in its failure, as a snapshot test does. A short hash can still come from that text.

## Open questions

- Which way the test fingerprints a check: by its tokens, its declared attributes, or its
  behaviour.

## References

- [Apache Avro 1.9.2 specification](https://avro.apache.org/docs/1.9.2/spec.html), "Schema
  Resolution"
- [Confluent Schema Registry, "Schema Evolution and Compatibility"](https://docs.confluent.io/platform/7.2/schema-registry/avro.html),
  and ["Data Contracts for Schema Registry"](https://docs.confluent.io/platform/7.8/schema-registry/fundamentals/data-contracts.html)
- [Google, AIP-180, "Backwards compatibility"](https://google.aip.dev/180)
- [Apache Iceberg, "Evolution"](https://iceberg.apache.org/docs/latest/evolution/)
- [Delta Lake, "Constraints and Checks"](https://delta.io/blog/2022-11-21-delta-lake-contraints-check),
  and [Databricks, "Type widening"](https://docs.databricks.com/aws/en/ingestion/cloud-object-storage/auto-loader/type-widening)
- [dbt, "Model contracts"](https://docs.getdbt.com/docs/mesh/govern/model-contracts), and
  ["Model versions"](https://docs.getdbt.com/docs/mesh/govern/model-versions)
- [buf, "Detecting breaking changes"](https://buf.build/docs/breaking/)
- Rust crates: [serde-reflection](https://docs.rs/serde-reflection),
  [type_hash](https://docs.rs/type_hash), [hashfn](https://docs.rs/hashfn),
  [nutype](https://docs.rs/nutype), [schemars](https://docs.rs/schemars),
  [facet](https://docs.rs/facet), and [facet-validate](https://docs.rs/facet-validate)
