# platform-core

What is where, and what is about to be crossed, as a Crux core.
[The Crux book](https://redbadger.github.io/crux/) explains the core-and-shell division, and
[managed effects](https://redbadger.github.io/crux/part-2/effects.html) how an effect is requested
rather than performed. What follows is lookout's own.

A shell reads a receiver, a browser's geolocation, a battery pin or a clock. Deciding what any of
that means happens here, where a laptop can test it. The same test on the board costs a cold start
under open sky, or an hour and a half of discharge.

The prediction itself is [`predictor`](../predictor/README.md)'s. This crate holds the state a shell
drives it with: the parser, the predictor and the battery.

## One core, a view each

A shell brings two answers to "which shell is this": where its crossings come from, and what it
shows. The device carries its set in flash and draws a 13-character panel; the browser fetches a set
and draws a canvas. Everything between — the parsing, the scan, the clock — is the same code either
way. There is no second copy of the prediction state to drift from this one.

The model's fields are private, so a projection reads it through methods and cannot reach past what
the core says it knows. The clock and the fix come from the predictor rather than being
held beside it, so no view can report a position the scan never ran from.

Everything is measured in `f32`. The browser would not mind `f64`, but one core means one float and
[the board is the one with no choice](../../docs/device.md#the-gnss-receiver).

## Nothing is drawn unless something moved

An event that moved nothing is answered with no effect at all. That matters because the receiver
bursts [a second's worth of NMEA sentences at a time](../../docs/device.md#the-gnss-receiver):
otherwise one position would scan the whole set and redraw the screen once per sentence.

A sentence that completes no fix changes nothing: one emitted before the receiver has a fix, one
whose checksum fails, one repeating what is known. Nor does a voltage landing in the charge step
already showing, a set arriving for a predictor that already has one, or an event the predictor
refuses as out of order.

## A core with no crossings has no predictor

A predictor is built from its crossings and keeps them for as long as it lives. A core with nothing
to scan therefore holds no predictor, rather than one scanning an empty set. Everything measured
against the set — the clock, the fix, the predictions — arrives with it. Anything a shell sends
before then has nowhere to be measured, and is refused.

`Reset` is the one event answered with more than a render. It puts the model back to what it was
built as, which is also what decides whether there are crossings to ask for: a shell that carries
its own is asked for nothing, and a shell that fetches them fetches again.

A shell sends it on coming up, and again whenever what follows has nothing to do with what came
before. The kiosk is that case: the next journey happened before the last one, so every fix in it
would otherwise be refused as stale.

The request for a set carries nothing, since which set to send is the shell's business and a shell
reading flash has nothing to look up. Its answer comes back as an ordinary event rather than as that
request's response, so fetching a set need not hold the request open.

## What the events mean

A tick is the time as the shell reads it, so a countdown shortens between fixes. A tick behind what
the receiver has already reported is refused. The device has neither NTP nor an RTC, so its clock is
the receiver's: it can send ticks or not, and the panel reads the same either way.

A sentence arrives as text. A position arrives parsed, and carries its own instant, since a fix is
dated by whatever produced it rather than by the shell. A position off the globe is refused exactly
as a corrupt sentence is: the last fix and its predictions stay where they were.

A battery event is the terminal voltage in millivolts. What it means is decided here, not in the
shell, and a shell with no battery to read never sends one.

## How full the battery is

The voltage-to-charge curve comes from `battery-estimator` rather than being fitted here. The
board's cell is lithium-polymer, which sits near 3.7 V for most of its life and then falls off a
cliff. Interpolating between empty and full would therefore read half-full for most of a discharge,
then drop three bars at once. The crate's curve for that chemistry is 3.2 V empty, 3.7 V nominal,
4.2 V charged.

The estimator clamps rather than refuses, having no out-of-range error. So a voltage outside what
could plausibly be a battery here is dropped before it reaches the curve. A disconnected pin reading
near zero would otherwise come back as a confident 0%, and a misread of 9 V as a confident 100%;
saying nothing is better than saying "full".

Charge is reported in as many steps as the indicator can honestly claim. The curve is flat through
the middle of a discharge, so finer steps would report noise as information. A step holds
its place until the reading moves clear of its boundary. A reading sitting on one would otherwise
flicker between two steps every time it is taken, and the panel redraws on every change.

## Reading the packed set in place

The device holds the whole set in flash and scans it against every fix, so nothing here copies or
allocates: the columns are the file's own bytes, cast where they lie. `include_bytes!` yields a
buffer aligned to 1, so packed bytes go behind a type that forces the four-byte alignment the
columns are cast at. An unaligned buffer is refused rather than faulting on Xtensa.

A coordinate off the globe is found once, here, rather than being measured against every fix for the
rest of the run.

A set built into a binary is checked at compile time as well. The embedded bytes face the same three
questions: the magic, the version, and whether the length matches the count claimed. A set this
reader cannot make sense of therefore stops the build rather than reaching a device.

This is the second implementation of that format. [`crossings`](../crossings/README.md) packs the
file and defines the layout, but it reads GeoParquet through arrow and could never build for this
target, so the constants are repeated rather than shared. What makes that safe is
`tests/four-crossings.pointset` — a real file from the packer — and the test that reads it.
