# Where the domain lives

*Assessment of 2026-09-21, written after the crossing types moved to `model` and before
anything else did. It records what a sweep of `medallion-model` found and what it
recommends, not a change. Whatever survives being acted on belongs in a durable doc; the
rest goes stale.*

## The criterion

A type belongs in `model` when it says what something in lookout *is*, in terms a store and a
device would both recognise. Not "something that cannot build arrow needs it" — that is a
symptom, and a narrow one: it finds a type only once a device has already been written that
imports it, which is one platform too late.

The bias runs the other way. A new entity is described in `model` by default, and a crate
that keeps, derives or draws one refers to that description. What stays out is what a
particular consumer does with it.

## What a row is, then

Not the entity. `medallion-model` holds how the domain is laid out in the store — which
datasets exist, how they are partitioned, what columns they have — and a row type is the
entity's projection into columns, with the store's own additions: the partition keys, the
derived denormalisations, the tuning a run was made under.

That relationship already exists and works. `CrossingCompact` is the entity;
`CrossingCompactRow` is its three-number projection; the conversion between them lives with
the entity and nothing else knows the wire shape. The store's rows are the same idea against
a different encoding.

This matters because a row cannot simply *be* an entity. `medallion::fields` traces schemas
with `serde_arrow`'s `from_type`, which documents `#[serde(flatten)]` as unsupported — it
turns a struct into a map. Embedding without flattening traces to a struct column and renames
every column to `gps.lat`. So a row type stays a flat struct, and the entity it projects is a
separate type with conversions. Restating the fields is the mechanism; the entity is what
stops the restatements drifting.

## What is described only as a row today

| Entity | Described in | Also shaped as |
| --- | --- | --- |
| Session | `SessionRow` | `matching::Session`, `gold::Replay` |
| Sample (a fix with its instant) | `SessionSampleRow` | `gold::Fix`, `Event::Position`, `matching::Sample`, `predictor::Sample` |
| Pass (a crossing met in a session) | `SessionCrossingRow` | — |
| Device | `DeviceId`, `DeviceSessionRow` | `shared::DeviceInfo`, `DeviceType` |
| Leg | `MotisSegmentRow`, `TrainSegmentRow` | `motis::client` types |
| Extract | `ExtractManifestRow` | `transport::extract` |

Value types in the same position: `OverlapKind` (how a track and water meet), `StartedBy` (why
a session began), `Bbox` (a window). Each is a fact about the domain, declared in the store's
schema.

Against that, `model` holds a crossing, a fix, a measure and a position. The domain is mostly
somewhere else.

## What the coupling costs

`session_crossings::matching` is a pure geometric algorithm — envelope prune, Euclidean
distance, nearest sample. It imports `medallion_model::{DeviceId, SessionCrossingRow,
SessionId}` and `passes()` returns `Vec<SessionCrossingRow>`. A calculation about geometry is
written in the store's column names, so it cannot be used by anything that is not writing to
the store, and a change to the dataset's columns reaches into it.

That is the shape to avoid, and it is the argument for the default. The pass it computes is
the unit the whole evaluation counts in (see
[2026-08-01-evaluation.md](2026-08-01-evaluation.md)) and has no type of its own anywhere.

## Recommendations

**Adopt the default, and realise it where a second shape already exists.** A second shape is
the evidence an entity is real; adding one before that is a type with no caller, which is the
objection that kept a long-id `Crossing` out of `model` until two derivations wanted it. In
order:

1. **`Sample { t, gps }`.** Already written three times — `gold::Fix`, `Event::Position`'s two
   fields, `matching::Sample`. The smallest possible move, and it is the entity everything
   downstream is derived from.
2. **`Pass`**, with `SessionCrossingRow` as its projection, so `matching` computes passes and
   the store writes them. This is what decouples the algorithm.
3. **`Session`**, holding the device, the span and the count; the row adds the envelope and the
   tuning.
4. **`DeviceType` into the row.** `DeviceType::as_str` exists, per its own doc, "for storing in
   a text column", and has one caller. It is unnecessary: `medallion::fields` traces with
   `enums_without_data_as_strings(true)`, so a fieldless enum already stores as its own name —
   `OverlapKind` does exactly this, with a test asserting the column is Utf8. Moving
   `DeviceType` to the domain lets the row name it and deletes `as_str`.
5. **One checked window.** `crossings::Bbox` validates on-globe, west-of-east, south-of-north
   and parses from a string; `medallion_model::Bbox` validates nothing, and
   `session_crossings` converts it to the `Rect<f64>` the other already wraps. A stored
   envelope with `xmin` past `xmax` prunes every session silently. The checks are the window's
   own, so they belong with it in the domain.

**Rename the crate to `domain`.** `model` beside `medallion-model` reads as two peers, and
`model` alone says nothing — a view model and a data model are both models. `domain` names
what the crate holds and what the other one is a schema *of*, and it reads at the call site:
`domain::Crossing`, against `domain_model::Crossing`. A short README stating the default above
is what stops the crate drifting back into a bag of portable types.

## Considered and rejected

**Flattening the entities into the store's rows**, so one description serves both. Ruled out by
the tracer, above.

**Renaming the store's columns to the fix's field names** (`accuracy_metres` for `acc`). The
short names are the wire's and the archive's — every recording ever made uses them — so the
rename would stop at the store boundary anyway, leaving the two descriptions it meant to merge.

**Moving `SessionId` for symmetry with `CrossingId`.** It moves with `Session`, when `Session`
moves, and not before.

## References

- [serde_arrow `SchemaLike::from_type`](https://docs.rs/serde_arrow/0.13.7/serde_arrow/schema/trait.SchemaLike.html)
  — the unsupported-feature list that rules out flattening
- [`medallion.md`](medallion.md) — what the store promises about identity and columns
