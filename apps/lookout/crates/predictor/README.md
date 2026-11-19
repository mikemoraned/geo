# predictor

What is coming up, from where we are and how fast we are going. Events in, and out each crossing's
state: out of reach, a candidate, alerted on, or passed. No store, no hardware, no clock of
its own, so the same code runs on the board, in a browser, and in a replay of a recorded session.

Three parts turn the events into states, each behind a trait so another can replace it:

- **A predictor** (`Predict`) predicts each crossing within reach, with its distance, its arrival,
  and a confidence of low or high.
- **A pass detector** (`DetectPasses`) reports the crossings being passed.
- **The tracker** (`Tracker`) turns each round of predictions and passes into each crossing's state,
  and records every transition between states.

## CrowFlies is the simplest predictor

A distance here is the great-circle line to the crossing, and an arrival is that distance divided
by the speed estimated from the fixes so far. The track plays no part, so a curve or a river
meander puts a crossing nearer, and sooner, than the rails can reach it.

CrowFlies predicts every crossing within its watching radius. It predicts with high confidence a
crossing within its alerting radius that it expects to reach within its horizon, and with low
confidence every other crossing it predicts. The watching radius is how far ahead to look, and
reaches at least as far as the alerting radius. The defaults are 5 km to watch, and 1 km and
5 minutes to alert.

An arrival is an instant rather than a countdown, so it stays true while the clock advances
between fixes. A shell wanting a countdown subtracts the time it is showing it at.

A test checks a distance against a degree of latitude worked out independently on the same
mean-radius sphere, so the formula is what it tests, not the geometry crate agreeing with itself.

## LightCone keeps what lies ahead

LightCone wraps CrowFlies, and keeps its high confidence only for a crossing that lies within a
cone around the heading. The cone's half-width is the largest angle between the heading and the
bearing to the crossing, 45° by default. A crossing outside the cone, such as one the train has
already passed, drops to low confidence. LightCone never
raises a confidence, so it alerts on a subset of what CrowFlies alerts on.

## Each crossing has a state

A crossing's state is a type of its own, and a crossing moves between states only through a method
of the state it is in:

| From | Method | To |
| --- | --- | --- |
| Candidate | `alert` | Alerted |
| Alerted | `pass` | Passed |
| Alerted | `withdraw` | Candidate |
| Passed | `alert` | Alerted |
| Passed | `release` | Candidate |

Out of reach is the absence of a state. A crossing leaves it for a candidate or for alerted, and
returns to it from any state. The types make every other transition unrepresentable, so the
tracker cannot record an illegal one.

## The tracker turns predictions and passes into states

At each round, the tracker takes the predictions and the passes, and moves each crossing:

- A crossing predicted with low confidence becomes a candidate, and one predicted with high
  confidence becomes alerted.
- An alerted crossing that is being passed becomes passed. One predicted with high confidence and
  being passed in the same round goes through alerted to passed, and both transitions are
  recorded at that round.
- A passed crossing stays passed while it is being passed, whatever is predicted. Once it is no
  longer being passed, the prediction decides its state again.
- A crossing no longer predicted returns to out of reach.

Entering the alerted state is a notification. The tracker refuses a round dated before its last,
as the predictor refuses an event.

## Pass detectors

A pass detector observes the same events as the predictor. `OnlineDetector` reports each crossing
within its pass radius of the latest fix, 250 m by default. It sees only the fixes so far, as a
device does. A replay can take its passes from a detector that sees the whole session instead,
which the evaluation does to measure how far the two disagree.

## The crossings are a source, not a slice

A platform holds its set the way that suits it. The board keeps thousands in flash as parallel
columns and scans them where they lie, since copying them into a `Vec` would cost RAM it has
better uses for; off the board a `Vec` is the obvious source. So the predictor asks its source for
every crossing, in whatever order it holds them — a scan reads all of them, so the order is the
source's to choose.

## Time only goes forwards, and a fix is ordered against a fix

An event dated before the clock is refused, and refusing changes nothing: the clock, the
predictions and the speed are left as the last accepted event left them. An event at the clock's
own instant is accepted, because one fix arrives as several sentences bearing the same epoch, each
saying more than the last.

A fix, though, is ordered against the fix it replaces rather than against the clock. What makes a
fix stale is a newer fix, and the clock can have run on without one — a browser's fix is stamped
seconds before it is delivered, so a clock refusing anything behind it would throw away every fix
that followed a tick.

## The speed comes from the fixes, not the receiver

A velocity estimator turns the fixes into a speed and a heading, and a predictor takes it as a
parameter, so a better estimator replaces the default without touching the predictor. The speed a
receiver reports plays no part. A phone's geolocation often leaves it out, so ignoring it on every
platform keeps the estimate alike across them.

The default estimator measures the last move between two fixes further apart than a radius of
stillness. A receiver at rest wanders a few metres between fixes, and a move inside that radius
would give a speed, and a heading, where there is none. The speed is the move's length over the
time from its start to the latest fix, so it falls while the train stands still.

At `f32` an estimated speed is less exact, and the slower we go the less exact it gets: `f32`
resolves latitude to about 0.42 m, so each fix carries that much error and so does the step
between two of them. A train covers 30 m in a second, putting the error near 1%; walking pace
covers 1.4 m, putting it near 30%. At `f64` it does not arise. At a standstill there is no arrival
to predict: we never arrive.

## Sentences are the board's way in

A fix is built from what a receiver has said so far rather than from the sentence last read, since
[one fix is spread over several sentences](../../docs/device.md#the-gnss-receiver) and only one of
them carries a date. Three kinds of sentence add nothing and leave what is known intact: one whose
checksum does not match its body, one emitted before the receiver has a fix, and one that repeats
what is already known.

A sentence promises its shape and nothing more — `$`, a body, `*`, two hex digits. Whether the
checksum matches is the parser's question, since the NMEA reader verifies it while reading the
fields, and checking it in both places would put one rule in two, to disagree in one of them. That
matters in practice: a captured overrun spliced two sentences into one well-formed sentence whose
checksum belonged to neither half.

Sentences in the shape the receiver emits them sit behind the `fixtures` feature, so none reaches
a device binary; a crate wanting them puts `predictor` in its `[dev-dependencies]` with the
feature on. One place knows what a sentence off this receiver looks like — the field count, the
trailing pair NMEA 4.1 adds, the empty course a stationary receiver reports — so a test needing a
fix somewhere else asks for one there rather than writing another sentence out. The constants are
[captures with the position replaced](../../docs/device.md#the-gnss-receiver), and tests hold the
builder to them, so what it builds is what the receiver sends.
