# Spike 0 — toolchain + flash

Proves the Xtensa `std` path end to end on an **M5StickC PLUS2** (ESP32-PICO-V3-02):
scaffold → build → flash → serial log. Nothing here is meant to be reused directly; later
spikes build on what it established.

## What it does

- Drives **G4 (HOLD) high** as soon as it has taken the peripherals, and keeps the
  driver alive for the life of the program. Without this the PLUS2 cuts its own power
  immediately once it is on battery rather than USB.
- Logs `hello …` and the PSRAM size over serial, then a `tick N` heartbeat every second.
- Blinks the red LED (G19, active-low) once a second — the only liveness signal once USB,
  and with it the serial console, is unplugged.

## Running

```sh
just flash     # build, flash, and tail the serial console
just monitor   # tail the console of whatever is already flashed
```

`espflash` picks the serial port itself; set `ESPFLASH_PORT` to pin it to one device.

## What it established

The toolchain and the board facts this and every later spike rest on — the `esp` channel and
`LIBCLANG_PATH`, PSRAM needing no PICO-specific overrides, G4 as the hold pin, and the LED —
are in [`docs/device.md`](../../../docs/device.md).
