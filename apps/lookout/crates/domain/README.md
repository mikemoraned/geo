# domain

What lookout handles — `Crossing`, `Sample`, `Pass` — and the names they go by. The decisions are
elsewhere: what to predict, what to keep, what to draw. A change here reaches all of them.

Types here construct, convert and report; they decide nothing. The crate builds for Xtensa and for
wasm, so nothing reads a store, drives hardware or asks the time.

## A few things have identity; the rest are their values

A `Crossing` is the same crossing after a rebuild moves it a few metres, so it carries a
`CrossingId` that outlives the change. A `Gps` and a `Bbox` are what their fields say.

A name belongs here even where the thing it names does not: there is no `Session` and no `Train`,
only `SessionId` and `TrainNumber`. A crossing has two names, since the places holding it have
different room, and whoever holds one never derives the other.

A name refuses what names nothing. `CrossingId` and `SessionId` refuse the characters a directory
or a query would misread; `TrainNumber` refuses a zero. That rule belongs to the name, not to
whatever stores it.

## Coordinates are degrees, checked once, held at two precisions

Degrees arrive as `f64` from every source — a parsed sentence, a stored column, a browser's fix —
and convert once, where they are checked. A coordinate off the globe is refused there, and the
error names the `f64` that was wrong rather than what it became.

A position is held at the precision its platform works in: `f64` where a source reported it and a
store keeps it, `f32` on [the board](../../docs/device.md), whose FPU is single precision. A type
parameter rather than two types, since a crossing and a fix measured differently cannot be
subtracted.

In memory a position is georust's own: `x` is longitude, `y` is latitude. Written in degrees — on
a wire, in a positional row, in a constructor taking two numbers — latitude leads. The two orders
disagree and both axes are the same type, so a swap is silent; nothing builds a point by hand, and
a test at each form asserts which number came out where.

What arrives is read as written: one impossible coordinate is a row to drop, not a reason to
refuse the archive holding it. The check happens on the way into the precision a scan runs in,
before such a position is subtracted from every crossing in the set. A crossing read from a buffer
that can arrive corrupt is checked on the same terms.

## A source reports what it has, and absence is kept

A fix carries a position; everything beside it is optional. A phone reports accuracy in metres and
counts no satellites; a receiver reports satellites and HDOP and judges no accuracy. An absent
heading is a device standing still rather than one that does not know, so it stays absent.

A caller sets what the source knew one field at a time, each method naming the field it sets, so
no call site depends on the order of two numbers of the same type.

## An id is derived from what the thing is

A session's id is a name-based UUID over the device and the instant it began, so a run re-deriving
a session it has already written lands on the same id and rewrites it in place. The namespace is
its own, and a test against a session the store holds pins both that and what is hashed: either
could change, derive consistently, and rename every session recorded.

A crossing's id is derived from the water, the stretch of track, and where along the track the two
meet. The place is part of it because a line following a valley crosses the same river repeatedly,
and those are separate crossings. The compact name is minted alongside, where a collision between
two crossings can still be refused; downstream, a packer, a scan and a page read the name they
were given.

## A window includes its edges

A window selects points rather than partitioning space, so a point exactly on a boundary someone
drew is one they meant. Underneath it is georust's `Rect`, which orders its own corners, so a
window built either way round is never inside out.

## What was found, and how hard something looked, are different things

A `Pass` says which crossing a session passed, when, and on what evidence — how far the nearest
sample was, and how many samples fell close. A crossing matched by one distant sample and one
matched by twenty close ones are both passes, and a reader weighs them. The radius the run
searched within is not part of it: that is how hard it looked, and belongs with the record of the
run.

## A stored or sent shape is a projection

Whoever keeps or sends something holds a projection of it: the fields flattened, renamed,
positional, or narrowed to the precision that consumer works in. The conversion lives with the
type, so two projections cannot drift. Three things call for one:

- A format already written. `Gps` names its fields in full; the wire keeps the short names the
  archive holds.
- Size, where it costs something. `CrossingCompact` is named in four bytes and sent as three bare
  numbers; `Crossing` is neither.
- A store deriving its columns from a row type, as [the medallion
  store](../../docs/medallion.md) does. Those columns are flat, and tracing them probes the type
  with values, so one that refuses a probe describes no column: `TrainNumber` holds a `u32` and
  refuses zero where one is made.

`Crossing` has no projection because nothing sends or stores it. What crosses a wire or fills a
buffer is `CrossingCompact`, and a set of thousands goes as an array of `[id, latitude,
longitude]` — a third the size of named rows. A set is read unchecked, so a reader drops the rows
it cannot use rather than losing the set to one of them.

Reading a name that names nothing answers `None` rather than an error, where nothing downstream
needs the value: a trip naming no train is ordinary, not a failure.
