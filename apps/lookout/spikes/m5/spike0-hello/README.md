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

- **Toolchain.** `espup`'s `esp` rustup channel (a nightly-based 1.90 fork) plus
  `esp-idf-template` targeting `xtensa-esp32-espidf`, ESP-IDF v5.5.3. Edition 2024 is fine.
- **LIBCLANG_PATH is required.** Without the Xtensa clang from
  `~/.rustup/toolchains/esp/xtensa-esp32-elf-clang/*/esp-clang/lib` on `LIBCLANG_PATH`, the
  `esp-idf-sys` bindgen step fails with `unknown target triple 'xtensa'` *after* a full
  ESP-IDF build, so the real cause is a long way up the log. The `Justfile` resolves it.
- **No `[patch.crates-io]` needed.** The template patches `esp-idf-sys`/`-hal`/`-svc` to git
  HEAD; the released `esp-idf-svc` 0.52.1 builds fine, so this spike drops the patch in favour
  of something reproducible.
- **PSRAM works with the stock quad-SPI settings.** ESP-IDF identifies the package itself
  (`quad_psram: This chip is ESP32-PICO-V3-02` → `Found 2MB PSRAM device`) and adds the
  2048K pool to the heap allocator; no PICO-specific pin overrides are needed.
  `CONFIG_SPIRAM_IGNORE_NOTFOUND=y` is kept as insurance — a failed probe then reports as
  `psram: 0 bytes` in the startup log instead of aborting boot.
- **The board is 8MB flash, chip revision v3.1**, and `esp-idf-svc` binds the legacy I²C
  driver by default (`W i2c: This driver is an old driver …`) — worth
  resolving when spike 2 reads the BM8563 RTC over I²C.
