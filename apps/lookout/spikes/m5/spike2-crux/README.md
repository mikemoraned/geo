# Spike 2 — Crux on device

A Crux core holding the current time, rendered to the screen by a Rust shell that ticks
once a second. Builds on spike 1's display.

```sh
just test      # run the core's tests on the laptop
just flash     # build, flash, and tail the serial console
```

## Why two crates

`esp-idf-sys`'s build script aborts on a host target, so **any** crate that depends on
`esp-idf-svc` can never be built or tested off-device. Host-testability is the main reason
Crux is here at all, so the split is forced:

- `core/` — the app: `Model`, `Event`, `Effect`, `view()`. No `esp-idf-*` dependency, so
  `cargo test` runs it on the laptop against the same source the device flashes.
- `shell/` — the device half: display setup, the once-a-second tick, and carrying out the
  effects the core asks for.

The shell imports the core as a plain path dependency. Crux's typegen and `Bridge` exist
for non-Rust shells (Swift, Kotlin); a Rust shell needs neither.

## Shape

The shell never decides what the screen says. It reports a `Tick(now)` and the core answers
with a `Render` effect and a view model holding the formatted string. That is the same
division the predictor will need: the interesting logic testable on the laptop, the device
crate reduced to I/O.

## Time

The shell reads `SystemTime`, which without NTP or an RTC read counts from the epoch at
boot — it ticks correctly but the date is not real. Reading the BM8563 RTC was the
alternative; spike 3's GNSS fix carries true UTC anyway, which makes it the better source,
and this spike needs neither to prove the core/shell loop works.

`chrono` is built with `default-features = false` throughout: the default `clock` feature
drags in `iana-time-zone`, which has no business on the device, and the core is handed its
time rather than reading it.
