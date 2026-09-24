# The browser

The same core the board runs, compiled to WebAssembly by `wasm-pack`. The board's
counterpart is [device.md](device.md).

Hand-written pages, no framework and no build step, so a page reaches the core over JSON rather
than through a generated binding.

A Web Component owns the core and draws it, repainting when the core asks. A page adds it by
writing a tag and sends it positions and time; where those come from is all that separates
one page from another.

## The element holds the core, the page holds the source

Prediction, clock discipline and formatting are all the core's, and the element has only I/O:
it loads the wasm, answers what the core asks for, and paints what it says. An event that moved
nothing comes back with no request, so the element does not repaint — which is what stops a replay
at speed from redrawing per sample.

The wasm load sits at module scope, so several elements on a page share one, and the first paint
waits on its promise: a browser cannot await `connectedCallback`.

A page sends positions and time through one method, which is what lets one element serve a live
receiver and a replay. The canvas is sized at the device's own pixel ratio, so the dots are not
blurred on a phone.

## What the picture means

We are in the middle, and what is about to be crossed is around us.

```
                . - - - - .
             .               .          *   a crossing the core predicts,
           .      . - - .      .            drawn where it really lies
          .     .    *    .     .
          .     .    ●    .     .       ●   us, sized by the speed the core
          .     .         .     .           reckons we are going
           .      . - - .      .
             .          *    .          the outer ring is the furthest the
                . - - - - .             core looks; nothing is drawn past it
```

An azimuthal equidistant projection centred on the fix places each crossing, so it is drawn in the
direction it really lies; only how far out is remapped.

How far out comes from a symmetric-log scale over the core's own distance, with the furthest the
core looks landing on the rim: a dot there is a crossing at that maximum, and nothing is drawn
beyond it. Symmetric-log rather than log, because a crossing can be directly underneath us and log
has nowhere to put nought.

The scale's constant is how far out it stays close to linear. Below it a crossing moves across the
picture about as fast as it moves over the ground; above it distances compress. So the smaller the
constant, the more of the picture goes to what is close. At a constant of 500 m and a maximum of
5 km, a crossing sits this far out from the centre:

| distance | 250 m | 500 m | 1 km | 2 km | 5 km |
| --- | --- | --- | --- | --- | --- |
| of the way to the rim | 17% | 29% | 46% | 67% | 100% |

The inner ring is drawn half way out, which under those settings is a crossing about 1.2 km away.

The centre dot is sized by how fast the core reckons we are going. The core measures in metres a
second, which is what a distance divided by a time comes out in, and a page shows km/h, which is
how a train's speed is read.

The screen shows what the core believes rather than what the browser could say for itself: the
clock is the latest instant anything reported, and the speed is the one the arrivals were worked
out at.

## The pages differ only in what they feed it

`/live` reads this browser's geolocation and ticks every second, since fixes arrive seconds apart
and a countdown should shorten in between. A fix is sent under its own timestamp rather than the
time it arrived: it says where we were when it was taken, and an arrival counted from it is counted
from the right instant.

`/kiosk` replays recorded journeys and sends no time at all — a replayed fix carries the instant it
was recorded at, which is the clock everything in the view is measured against. Each journey is
watched in a fixed span whatever it took to record, since a recording runs for hours and nobody
stands in front of a screen for that.

The clock decides which sample to send, rather than a timer counting them off: each frame asks how
far through that span it is and sends the sample that far through the journey, so a browser that
cannot keep up skips samples instead of falling behind. A fix not sent costs nothing, because the
one that is sent carries its own instant and the speed between them is still measured over the
interval that really separated them. Each journey begins by telling the core to start again knowing
nothing, which is what lets a journey from the morning follow one from that evening, and the page
runs the journeys in turn for as long as it is left open.

`/record` captures and sends; [the telemetry wire](telemetry.md) describes what it sends.

Every page shows the git hash of the build serving it, so a reader can match what is running to the
source it came from.

## What iOS Safari makes the recording page do

The recording page runs on a phone, and most of its shape is that phone's:

- **No UA client hints.** `navigator.userAgentData` is Chromium-only, so `platform` and
  `userAgent` are all a page has to classify a device by. Modern iPadOS reports `MacIntel` with a
  touch screen, which is how an iPad is told from a laptop. iOS spells its version `OS 18_5` and
  macOS `Mac OS X 10_15_7`, so the page turns underscores into dots.
- **A screen wake lock, taken again when the page returns.** Without one iOS auto-locks, which
  suspends the page and stops capture. The lock is released whenever the page hides, so the page
  takes it again on becoming visible. It cannot survive the power button, and Low Power Mode
  refuses it outright — which the page says on screen, so a failure is visible on the train.
- **Motion needs a gesture.** Safari grants `DeviceMotionEvent` permission only from an explicit
  user action, so recording starts from a button.
- **`event.acceleration` is always present**, gravity removed, so no
  `accelerationIncludingGravity` fallback is needed. Its magnitude is orientation-invariant, so
  where the phone is sitting does not matter.
- **`POSITION_UNAVAILABLE` is usually transient.** `watchPosition` keeps trying after an error, and
  Core Location reports one while it has no fix yet, so an error neither ends the watch nor
  replaces a fix already held.
- **`pagehide` is the last event.** `beforeunload` and `unload` do not fire on iOS, so the page
  persists the outbox there.
