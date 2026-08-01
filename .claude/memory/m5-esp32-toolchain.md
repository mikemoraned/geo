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
- **Display SPI tops out at 26MHz, not the 40MHz M5GFX quotes.** The panel (SCLK 13, MOSI
  15, CS 5, DC 14, RST 12, backlight 27) is not on SPI2's IOMUX pins, so it routes through
  the GPIO matrix, which ESP-IDF caps at 80MHz/3. Over that, `spi_bus_add_device` fails and
  the device boot-loops on a confusing secondary assert in `SpiDriver::drop`.
- `esp-idf-svc` binds the **legacy** I²C driver (`W i2c: This driver is an old driver …`);
  worth resolving when reading the BM8563 RTC.
- **Grove UART: the Stick's RX is G33** (G32 is the other side), confirmed by listening on
  both at once. Community sources say G32 — they are wrong, and picking wrong looks exactly
  like a dead peripheral.
- **The main task needs far more stack than the template's 8192.** A shell with a display
  buffer, a UART buffer and a Crux core whose model embeds a parser peaked at ~26KB, so
  `CONFIG_ESP_MAIN_TASK_STACK_SIZE=32768`. Rust commits a function's whole frame on entry, so
  the size is set by everything `main` declares, not by what has run yet. An overflow does
  **not** report itself — it surfaces as a corrupted pointer inside whatever ESP-IDF call is
  deepest at the time, which sends you debugging innocent code. Log
  `uxTaskGetStackHighWaterMark(null)` at startup so there is a number to look at.
- **Never log from an `esp32-nimble` GATT callback.** They run on the NimBLE host task, whose
  default stack is 4096 (`CONFIG_BT_NIMBLE_HOST_TASK_STACK_SIZE`), and Debug-formatting a
  connection descriptor through the ESP logger overflows it. Keep callbacks to an atomic store
  and report from the main loop. Note it looks like it crashes *while idle*, because the host
  retries connections in the background and fires the callbacks unprompted.
- **Turn on `CONFIG_FREERTOS_WATCHPOINT_END_OF_STACK`.** Stack overflow has been the cause of
  every hard-to-diagnose crash on this board so far, and it never presents as one: it lands as
  a fault in whatever code is nearby (an SPI driver once, `memcpy` reading rodata another
  time, with an instruction-fetch-from-data `EXCCAUSE 2`). The watchpoint traps it at the
  moment of overflow and names the task.
- **`UartConfig`'s default `rx_fifo_size` is 256 bytes** (`UART_FIFO_SIZE * 2`) — too small
  for a GNSS receiver that bursts ~1.5KB of sentences once a second. Overruns are silent:
  they splice two sentences together so the checksum fails. 4096 is comfortable.

## The Xtensa backend crashes at random; just build again

`rustc` occasionally dies with `SIGSEGV` inside LLVM's Xtensa backend — seen as deeply
repeated `XtensaSizeReduce::ReduceMBB` frames, which reads as the compiler exhausting its own
stack rather than anything wrong with the code. **Re-running the identical build succeeds.**
Don't go looking for the offending source: an unchanged `just build` that failed and then
passed is the same input twice.

## Pin `crux_core` to `=0.16.2` on device

With `esp32-nimble` running, `crux_core` 0.19 reboots the device every 4s–7min, always inside
its per-effect `Command`/crossbeam machinery: a null `&self` in `CommandContext::clone`, or
endless recursion through `posix_memalign`. Without BLE the identical core runs indefinitely.

**0.16.2 is stable** (30 min, client connected, real fix). The port is one associated type —
`type Capabilities = ()`, which later versions dropped — and the `#[effect]` API is otherwise
identical. The cause was never found, only avoided: it is a change between 0.17 and 0.19.

Measurement ruled out stack overflow (watchpoint silent, high-water constant), both task
stacks, heap exhaustion and fragmentation (free heap and largest block flat to the byte up to
the crash), heap overrun (comprehensive poisoning silent), PSRAM, allocation volume, and
whether the model is boxed. Don't re-run those. Details, and the four confidently wrong
diagnoses that preceded the pin, are in `apps/lookout/spikes/m5/spike4-ble/README.md`.

## The GPS/BDS Unit v1.1 (AT6668)

115200 8N1, NMEA 0183 **4.1**, multi-constellation (`$GN*` with per-system
`$GP/GL/GA/BD/GQ GSV`). Its `RMC` carries a trailing mode/navigational-status pair that plain
0183 examples lack, and leaves the course field empty when stationary — fixtures written from
0183 documentation rather than from a capture get both wrong.

- It reports true UTC date and time in `ZDA`/`RMC` **before it has any position fix**, so the
  wall clock can be set from the receiver without waiting for a fix, and without the BM8563.
- `$GPTXT,01,01,01,ANTENNA OPEN` appears continuously **even with a good fix** — external
  antenna monitoring on a unit using its internal one, not a fault. Don't chase it.
- Noise characteristics, and what they mean for the predictor, are in the "Deploy predictor
  on M5 device" slice in `apps/lookout/docs/next-slices.md`.
- Captures are gitignored (`*.nmea`): a fix records where and when someone was.

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
host-targeted lookout workspace above it doesn't try to claim it. **A spike crate must
therefore never also appear in the app workspace's `members`** — the two together give cargo
two roots for one directory, and it then refuses to load *either*, so every `cargo` command in
`apps/lookout` fails with "multiple workspace roots found in the same workspace" (including
`just test`, which is how it goes unnoticed: the spike's own `just test` still works). A spike
core is reached through its own Justfile, never the app workspace. `esp-idf-template`'s
`[patch.crates-io]` git-HEAD pins are unnecessary — released `esp-idf-svc` 0.52.1 builds
fine, and edition 2024 works on the nightly-based `esp` channel. `Cargo.lock` **is**
committed, against the template's default: the spikes are kept as reference and have to
still build when someone returns to them.
