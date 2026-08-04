# The device

What the M5StickC PLUS2 and its GPS/BDS Unit do, established by running code on them. Check
here before assuming anything about the hardware: several entries contradict what the vendor
or the community documentation says.

## The board

ESP32-PICO-V3-02, chip revision v3.1, 8 MB flash, 160 MHz, 2 MB PSRAM, ESP-IDF v5.5.3.

There is no AXP192 on the PLUS2, so power-management code written for the StickC *Plus* does
not apply, and there is no PMIC to ask for a battery voltage.

- **G4 (HOLD) must be driven high to stay alive on battery.** Set it before anything else
  and keep the `PinDriver` alive for the life of the program: dropping it resets the pin and
  cuts power. On USB the board runs either way, which is how this is missed.
- **PSRAM needs no PICO-specific pin overrides.** Stock `CONFIG_SPIRAM=y` +
  `CONFIG_SPIRAM_MODE_QUAD=y` + `CONFIG_SPIRAM_SPEED_40M=y` gets all 2 MB; ESP-IDF
  identifies the package itself.
- The red LED is G19, active-low. Once USB is unplugged it is the only liveness signal
  besides the panel, because the serial console goes with it.
- **Battery voltage is GPIO38 on ADC1**, 12 dB attenuation, 12-bit, divider ratio 2.0, with
  line rather than curve calibration. These come from M5's own board table
  (`M5Unified/src/utility/Power_Class.cpp`, `board_M5StickCPlus2`), which is also where G4
  is confirmed as this board's hold pin.
- `esp-idf-svc` binds the **legacy** I²C driver (`W i2c: This driver is an old driver …`),
  which is worth resolving before reading the BM8563 RTC.

## Toolchain

espup's `esp` rustup channel (a nightly-based fork) with `esp-idf-template` targeting
`xtensa-esp32-espidf`. Edition 2024 works. `esp-idf-template`'s `[patch.crates-io]` git-HEAD
pins are unnecessary — the released `esp-idf-svc` builds fine, and a released version is
reproducible where a git HEAD is not.

- **`LIBCLANG_PATH` is required, and its absence is reported late.** `esp-idf-sys`'s bindgen
  step needs the Xtensa clang under
  `~/.rustup/toolchains/esp/xtensa-esp32-elf-clang/*/esp-clang/lib`. Without it the build
  fails with `unknown target triple 'xtensa'` *after* a full ESP-IDF build of around ten
  minutes, so the cause sits far above the error in the log. Glob the path in the recipe that
  builds rather than relying on a sourced shell profile.
- **The Xtensa backend crashes at random; build again.** `rustc` occasionally dies with
  `SIGSEGV` inside LLVM's Xtensa backend, seen as deeply repeated
  `XtensaSizeReduce::ReduceMBB` frames — the compiler exhausting its own stack, not a fault
  in the source. Re-running the identical build succeeds. A build that failed and then
  passed unchanged is the same input twice, so there is nothing to look for.
- **A crate depending on `esp-idf-*` cannot be host-tested at all.** `esp-idf-sys`'s build
  script aborts on a host target, with no "just build the lib" escape hatch. Anything to be
  tested on the laptop must live in a crate with no `esp-idf-*` dependency, which the device
  crate then depends on. This is what forces the core/shell split, not a preference for it.

### Device crates need their own workspace

A device crate builds for Xtensa and pins versions the host-targeted app does not, so it
declares its own `[workspace]` and carries its own lock. **It must then never also appear in
the app workspace's `members`.** The two together give cargo two roots for one directory, and
it refuses to load *either*. Every `cargo` command under `apps/lookout` then fails with
"multiple workspace roots found in the same workspace", while the device crate's own build
keeps working. That asymmetry is how the breakage goes unnoticed.

## Display

ST7789V2, driven with `mipidsi` + `embedded-graphics`. The configuration comes from M5's own
board definition in [M5GFX](https://github.com/m5stack/M5GFX/blob/master/src/M5GFX.cpp)
(`board_M5StickCPlus2`):

| | |
|---|---|
| SCLK / MOSI | G13 / G15 |
| CS / DC / RST | G5 / G14 / G12 |
| Backlight | G27 |
| Size | 135x240, **offset 52,40** |
| Colours | inverted |

- **The offset is the part that has to be right.** The controller addresses a window larger
  than the visible panel, so without it the image shifts and wraps.
- **The panel pins differ from the original StickC *Plus*** (DC 23 / RST 18 there), so
  Plus-era example code does not work here.
- **SPI tops out at 26 MHz, not the 40 MHz M5GFX quotes.** The panel is not on SPI2's IOMUX
  pins, so the signals route through the GPIO matrix, which ESP-IDF caps at 80 MHz/3 ≈
  26.67 MHz. Asking for more is rejected at `spi_bus_add_device`. The device then boot-loops
  on a secondary assert in `SpiDriver::drop`, tearing down a bus with a half-added device.
  The clock error above it is the real cause.
- **`mipidsi`'s `SpiInterface` is the right one**: 4-line serial, with a real DC pin. M5GFX
  sets `spi_3wire = true`, but that refers to its read path sharing MOSI and does not apply
  where nothing reads the panel.
- Drive the backlight high only after the first draw, so the panel never briefly shows
  whatever the controller powered up with.

## The GNSS receiver

GPS/BDS Unit v1.1 (AT6668) on the Grove port: 115200 8N1, NMEA 0183 **4.1**,
multi-constellation (`$GN*` sentences with per-system `GSV`). Cold start is around 23
seconds and needs sky view, so iteration on anything that consumes its output has to be
possible without it.

- **The Stick's RX is G33.** Community sources say G32 and are wrong. Choosing wrong is
  indistinguishable from a dead peripheral. The two pins are electrically independent, so
  listening on both at once and keeping whichever carries NMEA costs nothing.
- **`RMC` carries a trailing mode/navigational-status pair** that plain 0183 examples lack,
  and its course field is empty when stationary. Fixtures written from 0183 documentation
  rather than from a capture get both wrong.
- **True UTC arrives in `ZDA`/`RMC` before any position fix**, so the wall clock can be set
  from the receiver without waiting for a fix and without the BM8563 RTC.
- **`$GPTXT,01,01,01,ANTENNA OPEN` repeats continuously even with a good fix.** It is
  external-antenna monitoring on a unit using its internal one, not a fault.
- **`UartConfig`'s default `rx_fifo_size` is 256 bytes**, too small for a receiver that
  bursts around 1.5 KB of sentences once a second. Overruns are silent: they splice two
  sentences together so the checksum fails. 4096 is comfortable.
- **Test fixtures have to come from a capture, not from the specification.** Captures are
  gitignored, since a fix records where and when someone was, and a run of them is a movement
  trace. A fixture therefore keeps a real sentence's shape with its coordinates replaced,
  recomputing the checksum the change invalidates.

### Time without a fix, and without an RTC

`SystemTime` on this board counts from the epoch at boot unless something sets it: it ticks
correctly, but the date is not real. Since the receiver reports true UTC before it has a
position, it is the better source than the BM8563 RTC, and neither NTP nor the RTC is
needed.

Build `chrono` with `default-features = false` on device. Its default `clock` feature drags
in `iana-time-zone`, which has no business here, and a core that is handed its time rather
than reading it does not need it.

### What its numbers show

**Two observations, one short session each, in one place.** Read what follows as an
indication of the error that is possible, not as a measurement of the error to expect. The
sample is far too small to characterise the receiver, and nothing here has been repeated,
varied by location, or checked against a known-good reference.

Both were taken with the receiver deliberately held still, so any motion the numbers show is
error:

| geometry | position wander | receiver's own speed |
|---|---|---|
| 8 satellites, HDOP 2.4 | ~1.8 m over 12 s (~1.0 m/s implied by differencing) | 0.04–0.91 knots (≤0.5 m/s) |
| 6 satellites, HDOP 4.4 | ~4.5 m/s | 4.13 knots |

Two possibilities follow, both worth designing against because the precaution is cheap:

- **Differencing position between consecutive fixes can be dominated by noise at these
  scales.** If it is, a stationary device can appear to close on a crossing fast enough to
  emit confident nonsense. The receiver's Doppler speed, or differencing over a longer
  baseline than one reading, avoids depending on the answer.
- **The Doppler speed can degrade with fix quality too**, which is the more awkward of the
  two, since it is the fallback for the first. That argues for treating fix quality as an
  input rather than a diagnostic — `RMC`/`GGA` carry HDOP and satellite count — rather than
  for trusting the second reading above.

A train at speed swamps errors of this size either way. Approach and departure —
exactly when a crossing prediction is being refined — are the low-speed regime where they
would bite.

**What would settle it:** repeated stationary sessions across locations and sky views, and a
moving session against a reference the receiver is not the source of. Until then, treat a
predictor that behaves well here as untested. Check whether the phone traces show the same
thing before assuming any of it is specific to this receiver.

## Stacks, and how overflow presents

**Stack overflow has been the cause of every hard-to-diagnose crash on this board, and it
never presents as one.** It lands as a fault in whatever code is nearby: an SPI driver once,
`memcpy` reading rodata another time, reported as an instruction-fetch-from-data
`EXCCAUSE 2`. Turn on `CONFIG_FREERTOS_WATCHPOINT_END_OF_STACK`, which traps it at the
moment of overflow and names the task.

- **The main task needs far more than the template's 8192.** A shell with a display buffer,
  a UART buffer, and a core whose model embeds a parser peaks around 26 KB. Rust commits a
  function's whole frame on entry, so the size follows from everything `main` declares, not
  from what has run. Log `uxTaskGetStackHighWaterMark(null)` at startup so there is a number
  rather than an estimate.
- **Raising main's stack does nothing for a fault on another task**, which is what makes a
  NimBLE callback overflow so confusing.

## Bluetooth

NimBLE, via `esp32-nimble`, which needs `CONFIG_BT_NIMBLE_ENABLED=y` **and**
`CONFIG_BT_BLUEDROID_ENABLED=n`: Bluedroid is the ESP-IDF default, and leaving it on builds
the wrong host stack.

**Never log from a GATT callback.** Callbacks run on the NimBLE host task, whose default
stack is 4096, and Debug-formatting a connection descriptor through the ESP logger overflows
it. Keep a callback to an atomic store and report from the main loop. Two things disguise
this:

- Raising main's stack changes nothing, because main is a different task.
- It appears to crash while idle, because the host retries connections in the background and
  fires the callbacks with nobody touching anything.

## `crux_core` 0.19 rebooted the device; 0.16.2 is the fallback

**Build against the current `crux_core`.** What follows is a known-good version to retreat
to if the symptom below reappears, not a version to hold.

With `esp32-nimble` running, `crux_core` 0.19 rebooted the device every 4 seconds to 7
minutes, always inside its per-effect `Command`/crossbeam machinery — a null `&self` in
`CommandContext::clone`, or endless recursion through `posix_memalign`. Without BLE the
identical core ran indefinitely. On 0.16.2 the same build ran 30 minutes with a client
connected and a real fix.

The cause was never found, only avoided. It is some change between 0.17 and 0.19. Nothing
establishes that later releases still carry it: the versions after 0.19 are untried, and a
fault this visible may well have been fixed upstream since. The sequence on any new
work is therefore: build against the latest, soak it with BLE running, and drop back only if
the reboots appear.

Dropping back costs one associated type, `type Capabilities = ()`, which later versions
removed. The `#[effect]` API is otherwise identical. That is worth knowing when deciding how
much newer crux to lean on in a shared core. The retreat is cheap while the core stays close
to that API, and stops being cheap once it does not. That is a trade to make deliberately,
not a reason to write against 0.16.2 by default.

The fault is always inside crux's per-effect machinery. Two signatures, both reproducible:

```
crux_core::command::Command::new → Box::new_uninit → CommandContext::spawn
  → CommandContext::clone → crossbeam_channel::Sender::clone
LoadProhibited, EXCVADDR = 0x00000000, A2 = 0x00000000     ← null `&self`
```

```
Double exception, EXCVADDR = 0xffffffe0, backtrace an endless repeat of
posix_memalign / _DoubleExceptionVector, with crossbeam Receiver::try_recv on top
```

The first is a null pointer dereference in a context crux has just constructed. The second
is runaway recursion through the allocator. `App::update` returns a `Command` for every
event, and each one allocates channels, an `Arc`, and a slab entry, so this path runs
constantly.

**Measurement ruled these out — do not re-run them:**

| Suspect | Evidence against |
|---|---|
| Stack overflow | `CONFIG_FREERTOS_WATCHPOINT_END_OF_STACK` never fired; main's high-water a constant 22,812 bytes free of 49,152 |
| NimBLE host task stack | 6,492 of 8,192 free when callbacks run; raising it and removing all logging from callbacks changed nothing |
| Heap exhaustion or fragmentation | Free heap flat to ±8 bytes and largest block identical (110,592) across 7 minutes, up to the crash |
| Heap buffer overrun | `CONFIG_HEAP_POISONING_COMPREHENSIVE` ran across crashes and never reported a damaged block |
| PSRAM in the general heap | Same crash with PSRAM disabled entirely |
| Allocation churn | Cutting events ~15× (only `RMC`/`GGA` reaching the core) did not stop it |
| Model on the fragmented heap | Same crash with the core un-boxed, back on the main task's stack |

If a current version does reboot, three options remain untried: bisecting 0.17/0.18 to find
the change (one flash and a 15-minute soak each), porting to pre-`Command` crux 0.10, or
dropping crux from the device shell. The last forfeits the shared-core argument that put it
there.

## Battery

Read the pin in the shell and judge it in the core, as with NMEA: what a voltage *means*
belongs where it can be tested. A whole discharge, 4.2 V down to 3.2 V, then runs as a unit
test, asserting that the indicator never reads fuller as the voltage falls, that every step
is reachable, and that a reading on a boundary does not flicker. On hardware that experiment
takes an hour and a half and cannot be repeated on demand.

Show coarse steps rather than a percentage. A lithium-polymer cell sits near 3.7 V for most
of its life, so the middle of the curve is nearly flat and a percentage implies precision
the measurement has not got. Log the raw millivolts periodically, because the steps are too
coarse to check a divider or a calibration against.

Two things about the `battery-estimator` crate:

- Its `default_curves::LIPO` is **unreachable** — the module is private. `BatteryChemistry::LiPo`
  is the public way to the same curve.
- It **clamps out-of-range voltages rather than refusing**, and has no range error in its
  API at all. A disconnected pin reading near zero comes back as a confident 0%, and a
  misread of 9 V as a confident 100%. A plausibility range in front of it turns both into
  saying nothing.

`m5unified` knows this board properly, including a `battery_level()` for the PLUS2. But
`M5Unified::begin()` initialises the display too, with no power-only path, so one number
would cost the panel driver and a C++ ESP-IDF component. Worth revisiting if the IMU, RTC,
and buttons are ever wanted as well.

## Scanning the crossings

The device holds every crossing in flash and brute-force scans the lot against each GPS fix.
The packed format is described in
[`crates/crossings/README.md`](../crates/crossings/README.md).

**A scan of 5,749 crossings costs ~4.7 ms**, against a budget of the one-second gap between
fixes — 0.5% of it. Both figures below include parsing the sentence that carried the new
position, so they are upper bounds on the scan alone.

| profile | `opt-level` | `debug-assertions` | per scan | per crossing |
|---|---|---|---|---|
| dev | `z` | on | 7,835–8,377 µs | ~1.39 µs |
| release | `s` | off | 4,353–5,025 µs | ~0.82 µs |

- **Measure on release.** It is 1.7× faster than dev, and most of that is the
  `debug-assertions` and `overflow-checks` the dev profile turns on, which put a branch on
  every arithmetic operation inside the haversine. Both opt-levels are size-oriented.
- **Brute force stays settled well past this size**: around 120,000 points before a scan
  reaches a tenth of the fix interval, 1.2M before it fills it. The set would have to grow
  20× before an index is worth discussing.
- **Budget most of a microsecond per point on this board, not tens of nanoseconds.** At tens
  of nanoseconds 50k points would scan in single-digit milliseconds. At the measured rate
  they take ~41 ms.
- **The set costs flash, not RAM.** Nothing copies the columns out of flash: the reader
  borrows them where they lie. A scan therefore allocates only for what it reports, a few
  hundred bytes, and a bigger set would cost flash alone. Measured on the release ELF, the
  whole firmware is 764,694 bytes against 8 MB, of which 212,256 is the rodata carrying the
  69,000-byte set, with 7,882 bytes of static RAM.
- **Build the set into the binary rather than reading it from a filesystem.** At 69 KB
  against 8 MB the saving would be nothing. A filesystem would cost a partition table, a
  mount at boot, and a way for the device to end up holding a set that disagrees with the
  code reading it. That it is really there can be checked by searching the release binary
  for the format's magic and reading the header after it.
- **`f32` coordinates are enough.** Device and notebook agree to 0.27 m over 2.3 km, about 1
  part in 10,000, and count the same crossings within 5 km. The count is the stricter of the
  two checks, since a distance can be slightly out and still rank correctly where a
  membership question flips. The residual is what `f32` costs (~0.2 m at this latitude) plus
  the two implementations using slightly different mean earth radii. `i32` at 1e-7° would buy
  centimetres nobody can use, for a third more flash.
