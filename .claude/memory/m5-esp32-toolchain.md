# M5StickC PLUS2 / ESP32 toolchain

Working notes for the `apps/lookout/spikes/m5/*` spikes. Per-spike detail lives in each
spike's own README; this is what applies across all of them.

## Claude cannot flash the device

Every `/dev/cu.*` open returns `EPERM` from Claude's process tree — including with the
Bash sandbox disabled, so it is a session-level restriction with no workaround. Build and
verify in-session, then hand the user `! cd <spike-dir> && just flash` and read the boot
log they paste back. Plan device-facing work around that: put anything checkable into the
startup log (sizes, probe results) so one flash round-trip answers the question.

## `LIBCLANG_PATH` is required and fails late

`esp-idf-sys`'s bindgen step needs the Xtensa clang at
`~/.rustup/toolchains/esp/xtensa-esp32-elf-clang/*/esp-clang/lib` — espup puts it there and
`~/export-esp.sh` exports it. Without it the build dies with `unknown target triple
'xtensa'` *after* a full ~10 min ESP-IDF build, so the cause is far up the log. Each spike's
`Justfile` globs the path itself rather than relying on the shell having sourced anything.

## Board facts confirmed on hardware

- ESP32-PICO-V3-02, chip rev v3.1, 8MB flash, 160MHz, ESP-IDF v5.5.3.
- **PSRAM needs no PICO-specific pin overrides**: stock `CONFIG_SPIRAM=y` +
  `CONFIG_SPIRAM_MODE_QUAD=y` + `CONFIG_SPIRAM_SPEED_40M=y` gets all 2MB, with ESP-IDF
  identifying the package itself.
- **G4 (HOLD) high keeps the device alive on battery** — verified. Set it before anything
  else and keep the `PinDriver` alive for the life of the program; dropping it resets the
  pin and cuts power. There is no AXP192 on the PLUS2, so StickC *Plus* power-init code
  does not apply.
- Red LED is G19, active-low — the only liveness signal once USB (and the console) is gone.
- `esp-idf-svc` binds the **legacy** I²C driver (`W i2c: This driver is an old driver …`);
  worth resolving when reading the BM8563 RTC.

## A crate that depends on esp-idf cannot be host-tested

`esp-idf-sys`'s build script aborts with `Unsupported target 'aarch64-apple-darwin'`, so
there is no "just build the lib for the host" escape hatch. Anything that needs to run or
be tested on the laptop — notably the Crux core — must live in its own crate with no
`esp-idf-*` dependency, which the shell crate then depends on. Verified by building one
core for `xtensa-esp32-espidf` and testing the same source on the host.

`crux_core` 0.19.0 itself is fine on-target: a full `App` with `#[effect]`, `Command` and
`Core` compiles for `xtensa-esp32-espidf`. It requires rustc 1.90, which the `esp` channel
(1.90.0-nightly) satisfies. A Rust shell imports the core directly, so the `typegen`
feature stays off.

## Project shape

Each spike is its own cargo workspace (`[workspace]` in its `Cargo.toml`) so the
host-targeted lookout workspace above it doesn't try to claim it. `esp-idf-template`'s
`[patch.crates-io]` git-HEAD pins are unnecessary — released `esp-idf-svc` 0.52.1 builds
fine, and edition 2024 works on the nightly-based `esp` channel. `Cargo.lock` **is**
committed, against the template's default: the spikes are kept as reference and have to
still build when someone returns to them.
