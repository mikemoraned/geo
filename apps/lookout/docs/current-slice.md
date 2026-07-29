# Current Slice: Spikes on Device Support

### Target

Ultimately I'd like to be able to run the live predictor as an app installed on my ["M5StickC PLUS2"](https://shop.m5stack.com/products/m5stickc-plus2-esp32-mini-iot-development-kit?variant=44269818216705) with a ["GPS/BDS Unit v1.1 (AT6668)"](https://shop.m5stack.com/products/gps-bds-unit-v1-1-at6668?variant=45727253692673).

I'd like to use a series of Spikes to show this is possible by incrementally building a small app that can show current time + GPS reading (lat, lon) on the screen and exposed over BLE.

### Notes & Gotchas (hardware realities)

- **Toolchain = Xtensa `std` path.** ESP32-PICO-V3-02 is Xtensa: install the fork via `espup`, scaffold from `esp-idf-template` (target `xtensa-esp32-espidf`), flash + log with `espflash flash --monitor`. Confirms the "std not no_std" call.
- **`LIBCLANG_PATH` must point at the Xtensa clang** (`~/.rustup/toolchains/esp/xtensa-esp32-elf-clang/*/esp-clang/lib`, set by espup's `~/export-esp.sh`). Without it `esp-idf-sys`'s bindgen step dies with `unknown target triple 'xtensa'`, but only after a full ESP-IDF build — the cause ends up a long way up the log.
- **Crux is confirmed on-target** (`crux_core` 0.19.0, needs rustc 1.90 — the `esp` channel is 1.90.0-nightly, so fine). A full `App` with `#[effect]`, `Command` and `Core` builds for `xtensa-esp32-espidf`; no typegen feature needed for a Rust shell.
- **Split core and shell into separate crates.** `esp-idf-sys`'s build script aborts with `Unsupported target 'aarch64-apple-darwin'`, so *any* crate depending on `esp-idf-svc` can never be built or tested on the host. Host-testability — the reason Crux is here at all — therefore requires the core to be its own esp-free crate that the shell crate depends on. Verified both halves against the same core code.
- **HOLD pin (G4) HIGH at startup**, or the device shuts off the moment it's on battery instead of USB. Set it in the first lines of shell init. (PLUS2 has **no AXP192** — do not reuse AXP192 I²C power-init from StickC *Plus* examples.)
- **GPS on UART1/UART2, never UART0** (UART0 is the USB console). Grove port = G32/G33; wire GPS TX → Stick RX. Defaults: 115200 8N1, NMEA 0183.
- **Display needs an offset.** ST7789V2 135×240 sits inside a larger address window — give `mipidsi` the correct column/row offset or the image shifts/wraps. Pull exact display pins + offset from M5's schematic / Arduino board def; don't guess.
- **GPS cold start ≈ 23s, needs sky view** — no indoor fix. For desk iteration, replay recorded NMEA into the parser or sit by a window. Multi-constellation → sentences arrive as `$GN*`; enable `RMC` + `GGA` in the `nmea` crate.
- **BLE via `esp32-nimble`** on the std path. Expose lat/lon as a GATT characteristic with notify for the Spike 4 stream.
- **Enable PSRAM** (2MB on the PICO) in sdkconfig — crux_core + serde + NimBLE want headroom.
- **Battery ≈ 1–1.5h** with screen + GPS live; run field spikes off a USB-C power bank.
- **Core stays host-testable.** Keep behaviour in the Crux core so it runs/tests on the laptop with the same code as on-device — and so the predictor core from the predictor/eval slice can eventually *be* this on-device core. That reuse is the main reason Crux earns its place here.

### Straw Man

We should build a series of spikes in apps/lookout/spikes/m5. Each of these should be standalone but incrementally build on what was learned the previous:
0. **Toolchain + flash.** `esp-idf-template` project; log "hello" over serial. Proves espup/espflash/board. Set G4 HOLD here. *(std)*
1. **Hello on screen.** ST7789V2 via `mipidsi` + `embedded-graphics` — nail the offset — print "hello".
2. **Crux on device.** Rust shell imports the core directly (no typegen/FFI; that's only for non-Rust shells). Core holds `now` in its model; shell ticks once/sec and renders the view model. Time from BM8563 RTC (I²C) or esp-idf system time.
3. **GPS in.** First print raw NMEA over serial (shell only). Then parse with `nmea`, emit `GnssFix { lat, lon, .. }` into the core, render time + lat/lon.
4. **BLE out.** `esp32-nimble` GATT service; notify latest lat/lon as a sample stream.

### Tasks

**Spike 0 — Toolchain + flash**
- [x] Install Xtensa toolchain via `espup`, scaffold `spikes/m5/spike0-hello` from `esp-idf-template` targeting `xtensa-esp32-espidf`
- [x] Enable PSRAM in sdkconfig — stock quad-SPI settings are enough; ESP-IDF identifies the PICO-V3-02 itself and reports the full 2MB (`psram: 2097152 bytes`), no pin overrides needed
- [x] Hold G4 HIGH in the first lines of shell init so the device stays on under battery
- [x] Log "hello" over serial via `espflash flash --monitor`; confirm it survives unplugging USB — LED keeps blinking on battery, so the G4 HOLD holds

**Crux de-risk (before Spike 2)**
- [x] Confirm `crux_core` compiles for `xtensa-esp32-espidf`; record the outcome and any workaround in the notes above — it does; the workaround needed is the core/shell crate split, recorded above. Spike 2 should be laid out as `spike2-crux/core` (esp-free, host-tested) + `spike2-crux/shell` (esp-idf).

**Spike 1 — Hello on screen**
- [x] Pull exact ST7789V2 pins + column/row offset from M5's schematic / Arduino board def (don't guess) — from M5GFX's `board_M5StickCPlus2`: SCLK 13, MOSI 15, CS 5, DC 14, RST 12, backlight 27, SPI2 @40MHz, 135x240 offset 52,40, inverted. Differs from the original StickC *Plus* (DC 23, RST 18).
- [x] Drive the display with `mipidsi` + `embedded-graphics` and print "hello" correctly positioned — offset 52,40 confirmed correct on the panel. SPI had to drop to 26MHz: the panel isn't on SPI2's IOMUX pins, so the GPIO matrix caps it at 80MHz/3 and M5GFX's 40MHz boot-loops.

**Spike 2 — Crux on device**
- [x] Core holding `now` in its model, with host-side tests on the laptop — `spike2-crux/core`, 4 tests green via `just test`
- [x] Rust shell imports the core directly (no typegen/FFI), ticks once/sec and renders the view model to the screen — confirmed on the panel
- [x] Source time from BM8563 RTC (I²C) or esp-idf system time — took esp-idf `SystemTime`, which counts from the epoch at boot: it ticks correctly but isn't a real date. The RTC wasn't worth wiring up because spike 3's GNSS fix carries true UTC anyway.

**Spike 3 — GPS in**
- [ ] Wire GPS to the Grove port (G32/G33, TX → Stick RX) on UART1/UART2 and print raw NMEA over serial, shell only
- [ ] Parse `$GN*` `RMC` + `GGA` with the `nmea` crate; feed a recorded NMEA replay for desk iteration
- [ ] Emit `GnssFix { lat, lon, .. }` into the core and render time + lat/lon on screen
- [ ] Field-check a real cold-start fix with sky view (~23s), running off a USB-C power bank

**Spike 4 — BLE out**
- [ ] `esp32-nimble` GATT service exposing lat/lon as a characteristic with notify
- [ ] Confirm a phone/laptop client receives the live sample stream
