# m5

The M5StickC PLUS2 as a shell: `core` is what this board makes of
[`platform-core`](../../platform-core/README.md) — the crossings it carries and the panel it draws —
and `m5plus` is the pins and peripherals. Every board fact either of them rests on is
[device.md](../../../docs/device.md), established by running code on this hardware.

They are separate crates because a crate depending on `esp-idf-*` does not compile for the host at
all, and so cannot be tested. Everything the board decides therefore sits in `core`, where a laptop
runs it; `m5plus` is left with wiring.

## What fits on the panel

`FONT_10X20` is ten pixels wide, so a 135-pixel line holds thirteen characters:

```
              ┌─────────────┐
      y  20   │20:43:29[== ]│  the clock, then the battery in what is left
      y  42   │51.04030     │  where the fix put us
      y  64   │13.73220     │
      y  86   │6sat h4.4    │  satellites and HDOP, so a jittering distance
              │             │  can be told from a jittering fix
      y 108   │20 in 5km    │  how many crossings, and how far out it looked
              │             │
      y 130   │2.3km 1:24   │  each crossing: how far, then how long
      y 152   │2.3km 1:24   │
      y 174   │2.3km 1:24   │
      y 196   │2.3km 1:24   │
      y 218   │2.4km 1:24   │  the last one, clear of a 240-pixel screen
              └─────────────┘
```

Those are real values: Dresden Hauptbahnhof at 100 km/h, where the five nearest crossings are the
Elbe ones a train north out of the station passes, clustered within twenty metres of each other.

Nothing wraps, so a longer line runs off the side. A crossing's id will not fit beside the two
numbers and is the one to drop: it names a row in a dataset, where the distance and the countdown
are the prediction. A countdown of an hour or more shows as `>1h`, since hours would not fit and a
crossing inside the radius cannot honestly be that far off. The font is ASCII, so the battery is
bars in brackets rather than a glyph.

Every line is drawn padded to the full width, and every line the crossings can occupy is drawn on
each redraw, blank where there is nothing: the display erases nothing by itself, and the list
shortens as well as changes — five crossings to none when a fix is lost.

Only what changed is redrawn. A view model that has not moved costs no SPI traffic, and holding the
bus to redraw an unchanged screen takes long enough to lose incoming sentences.

The console log carries the raw voltage beside the bars, and the stack and heap figures beside
those: the bars are deliberately too coarse to check a divider or a calibration against, and a leak
tells itself apart from a level only as a number over a run.

## What it carries, and how it is pointed at a version

`include_bytes!` needs a literal path and a sized array, and both change with the version of the
packed set. The build script writes that declaration, taking the path from `crossings.version` and
the length from the file it names, so repointing the board is a one-line edit rather than a path and
a number that can disagree.

## Reading the receiver

Both candidate pins are opened and whichever carries NMEA wins, rather than trusting either source
that documents which one it is: choosing wrong is indistinguishable from a dead receiver, and the
pins are electrically independent, so listening on the idle one costs nothing. A probe waits a few
seconds — sentences arrive about once a second — and looks for the `$` that starts one, since any
bytes at all would pass on the noise an idle pin picks up.

Reads are short and blocking, so the loop keeps turning while the receiver is quiet, and a burst
arrives over several of them; the UART's own ring buffer is what has to hold a whole one. A line not
shaped like a sentence is dropped here, since a poor aerial produces those by the second.
