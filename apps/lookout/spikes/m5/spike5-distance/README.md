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

## What a scan costs: ~8 ms for 5,749 crossings

Measured on the board, against a real fix, `opt-level = "z"`:

```
I (4434) spike5_shell: carrying 5749 crossings
I (117424) spike5_shell: scanned 5749 crossings in 7836us
I (128244) spike5_shell: scanned 5749 crossings in 8377us
I (138354) spike5_shell: scanned 5749 crossings in 7998us
I (166354) spike5_shell: scanned 5749 crossings in 7835us
```

**7.8–8.4 ms**, so **~1.4 µs per crossing** — about 330 cycles at 240 MHz for one f32
haversine plus the top-N bookkeeping. The figure includes parsing the sentence that carried
the new position, so it is an upper bound on the scan alone.

Against a fix a second, that is **0.8% of the budget**. Brute force is settled at this size,
and stays settled a long way past it: ~72,000 points before a scan reaches a tenth of the
interval, ~700,000 before it fills it. The German set would have to grow **12× before an
index is worth discussing** and 120× before it is urgent.

### The prediction held, but its reasoning did not

The slice predicted single-digit milliseconds, and 8 ms is single-digit milliseconds. But it
got there via "even 50k points (~400 KB) scans in single-digit ms at 240 MHz", and at the
measured 1.4 µs a point, 50k would take **~70 ms** — an order of magnitude out. The
conclusion survived only because the real set turned out to be 9× smaller than the number the
estimate was built on. Worth remembering before reusing that reasoning for a bigger set: on
this board, budget microseconds per point, not tens of nanoseconds.

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

Comes from `just pack-crossings` in `apps/lookout` — see `crates/crossings/README.md` for the
byte layout. The device reader here is the second implementation of that format; the packer's
own round-trip tests are the first.
