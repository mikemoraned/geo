# Spike 5 — distance lookup

Shows what spike 3 shows — the clock and the current fix — plus the crossings nearest to
that fix, each with its distance. The point is to find out whether a brute-force scan of a
static point set is affordable on the device at the rate a GPS produces fixes.

```sh
just test      # run the core's tests on the laptop
just flash     # build, flash, and tail the serial console
```

Built on spike 3's core/shell split, for the same reason: a GPS needs sky view and a ~23s
cold start, so anything testable has to be testable without one.

## What a scan costs: ~4.7 ms for 5,749 crossings

Measured on the board against real fixes. Both profiles are worth knowing, because the one
you iterate on is not the one you ship:

| profile | `opt-level` | `debug-assertions` | per scan | per crossing |
|---|---|---|---|---|
| dev (`just flash`) | `z` | on | 7,835–8,377 µs | ~1.39 µs |
| release (`just flash-release`) | `s` | off | 4,353–5,025 µs | ~0.82 µs |

```
I (4306) spike5_shell: carrying 5749 crossings
I (201366) spike5_shell: scanned 5749 crossings in 4353us
I (317306) spike5_shell: scanned 5749 crossings in 5025us
```

**Release is 1.7× faster than dev**, and most of that is unlikely to be the opt-level — `z`
and `s` are both size-oriented — but rather the `debug-assertions` and `overflow-checks` the
dev profile turns on, which put a branch on every arithmetic operation inside the haversine.
Measure on release; the dev number is what iteration feels like, not what the device does.

At 0.82 µs a crossing — about 196 cycles at 240 MHz for an f32 haversine plus the top-N
bookkeeping — a scan is **0.5% of a one-second budget**. Both figures include parsing the
sentence that carried the new position, so they are upper bounds on the scan alone. (Four
samples on dev, two on release; the spread is small enough not to need more.)

Brute force is settled at this size and stays settled well past it: ~120,000 points before a
scan reaches a tenth of the fix interval, ~1.2M before it fills it. The German set would have
to grow **20× before an index is worth discussing**.

### The prediction held, but its reasoning did not

The slice predicted single-digit milliseconds, and 4.7 ms is single-digit milliseconds. But it
got there via "even 50k points (~400 KB) scans in single-digit ms at 240 MHz", and at the
measured rate 50k would take **~41 ms** — still an order of magnitude out. The conclusion
survived only because the real set turned out to be 9× smaller than the number the estimate
was built on. Worth remembering before reusing that reasoning for a bigger set: on this board,
budget most of a microsecond per point, not tens of nanoseconds.

## `crux_core` is pinned to `=0.16.2`

Not because this spike needs it — there is no BLE here, and the reboot only appears with
NimBLE running — but because the predictor this core is a step towards does. Building against
0.19 now would mean discovering on the way back that the core has to come down again. The pin
costs one associated type (`type Capabilities = ()`, dropped in later versions) and a `_caps`
argument to `update`. Spike 4's README has the evidence.

## The console

Spike 3 echoed every NMEA sentence, so a monitor session doubled as a capture. This one does
not: a dozen-plus sentences a second bury the one line worth reading, which is how long a scan
took. Fixtures for the core's tests came from spike 3 and have not changed, so nothing is lost
— go back to `spike3-gnss` and its `just capture` if more are needed.

## The point set

Comes from `just gold-pack-crossings` in `apps/lookout` — see `crates/crossings/README.md` for the
byte layout. The device reader here is the second implementation of that format; the packer's
own round-trip tests are the first.
