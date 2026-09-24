# predictor

What is coming up, from where we are and how fast we are going: the crossings within a radius,
nearest first, each with the instant we reach it. Events in, predictions out, and nothing else —
no store, no hardware, no clock of its own — so the same code runs on the board, in a browser and
in a replay of a recorded session.

## Crow flies, which is the baseline

A distance here is the great-circle line to the crossing, and an arrival is that distance divided
by the speed of the latest fix. The track plays no part, so a curve or a river meander puts a
crossing nearer, and sooner, than the rails can reach it. That is the point: it is a baseline
rather than an answer.

The radius is how far ahead to look — wide enough that a train at speed has a minute or two of
warning, narrow enough to mean something at walking pace.

An arrival is an instant rather than a countdown, so it stays true while the clock advances
between fixes. A shell wanting a countdown subtracts the time it is showing it at.

A test checks a distance against a degree of latitude worked out independently on the same
mean-radius sphere, so the formula is what it tests, not the geometry crate agreeing with itself.

## The crossings are a source, not a slice

A platform holds its set the way that suits it. The board keeps thousands in flash as parallel
columns and scans them where they lie, since copying them into a `Vec` would cost RAM it has better
uses for; off the board a `Vec` is the obvious source. So the predictor asks its source for every
crossing in the set, in whatever order it holds them — a scan reads all of them, so the order is
the source's to choose.

## Time only goes forwards, and a fix is ordered against a fix

An event dated before the clock is refused, and refusing changes nothing: the clock, the
predictions and the speed are all left as the last accepted event left them. An event at the
clock's own instant is accepted, because one fix arrives as several sentences bearing the same
epoch, each saying more than the last.

A fix, though, is ordered against the fix it replaces rather than against the clock. What makes a
fix stale is a newer fix, and the clock can have run on without one — a browser's fix is stamped
seconds before it is delivered, so a clock refusing anything behind it would throw away every fix
that followed a tick.

## The speed is the receiver's, or the step between two fixes

A source reporting no speed is ordinary: a phone's geolocation leaves it out, and a stationary
receiver reports no course to go with it. Two fixes say how fast.

At `f32` a derived speed is less exact, and the slower we go the less exact it gets. `f32` resolves
latitude to about 0.42 m, so each fix carries that much error and so does the step between two of
them. A train covers 30 m in a second, which puts the error near 1%; walking pace covers 1.4 m,
which puts it near 30%. At `f64` the error does not arise. At a standstill there is no arrival to
predict: we never arrive.

## Sentences are the board's way in

A fix is built from what a receiver has said so far rather than from the sentence last read, since
[one fix is spread over several sentences](../../docs/device.md#the-gnss-receiver) and only one of
them carries a date. Three kinds of sentence add nothing and leave what is known intact: one whose
checksum does not match its body, one emitted before the receiver has a fix, and one that repeats
what is already known.

A sentence promises its shape and nothing more — `$`, a body, `*`, two hex digits. Whether the
checksum matches is the parser's question, since the NMEA reader verifies it while reading the
fields, and checking it in both places would put one rule in two, to disagree in one of them. This
matters in practice: a captured overrun spliced two sentences into one well-formed sentence whose
checksum belonged to neither half.

Sentences in the shape the receiver emits them sit behind the `fixtures` feature, so none of them
reaches a device binary; a crate wanting them puts `predictor` in its `[dev-dependencies]` with the
feature on.

One place knows what a sentence off this receiver looks like — the field count, the trailing pair
NMEA 4.1 adds, the empty course a stationary receiver reports — so a test needing a fix somewhere
else asks for one there rather than writing another sentence out. The constants are
[captures with the position replaced](../../docs/device.md#the-gnss-receiver), and tests hold the
builder to them, so what it builds is what the receiver sends.
