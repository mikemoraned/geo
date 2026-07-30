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

## `crux_core` is pinned to `=0.16.2`

Not because this spike needs it — there is no BLE here, and the reboot only appears with
NimBLE running — but because the predictor this core is a step towards does. Building against
0.19 now would mean discovering on the way back that the core has to come down again. The pin
costs one associated type (`type Capabilities = ()`, dropped in later versions) and a `_caps`
argument to `update`. Spike 4's README has the evidence.

## The point set

Comes from `just pack-crossings` in `apps/lookout` — see `crates/crossings/README.md` for the
byte layout. The device reader here is the second implementation of that format; the packer's
own round-trip tests are the first.
