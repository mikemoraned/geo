# domain

What lookout handles — `Crossing`, `Sample`, `Pass` — and the names they go by. The decisions
are elsewhere: what to predict, what to keep, what to draw. A change here reaches all of them.

## What stays out

Types here construct, convert and report; they decide nothing. The crate builds for Xtensa and
for wasm, so nothing here reads a store, drives hardware or asks the time.

## A few things have identity; the rest are their values

A `Crossing` is the same crossing after a rebuild moves it a few metres, so it carries a
`CrossingId` that outlives the change. A `Gps` and a `Bbox` are what their fields say.

A name belongs here even where the thing it names does not: there is no `Session` and no
`Train`, only `SessionId` and `TrainNumber`. A crossing has two names, since the places
holding it have different room. Whoever holds one never derives the other.

A name refuses what names nothing. `CrossingId` and `SessionId` refuse the characters a
directory or a query would misread; `TrainNumber` refuses a zero. The rule belongs to the
name, not to whatever stores it.

## A stored or sent shape is a projection

Whoever keeps or sends something holds a projection of it: the fields flattened, renamed,
positional, or narrowed to the precision that consumer works in. The conversion lives with the
type, so two projections cannot drift.

Three things call for one:

- A format already written. `Gps` names its fields in full; the wire keeps the short names the
  archive holds.
- Size, where it costs something. `CrossingCompact` is named in four bytes and sent as three
  bare numbers; `Crossing` is neither.
- A store deriving its columns from a row type, as
  [the medallion store](../../docs/medallion.md) does. Those columns are flat, and tracing
  them probes the type with values, so one that refuses a probe describes no column:
  `TrainNumber` holds a `u32` and refuses zero where one is made.
