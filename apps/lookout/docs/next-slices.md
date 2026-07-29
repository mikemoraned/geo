# Next Slices

## Slice: minimal predictor and evaluation framework 

### Target

If other slices are done, we should have enough to put together a first minimal predictor that is based solely on crow-flies distance, and also to evaluate how good it is.

## Straw man

The essential idea here is to use our collected traces along with known water crossings to both act as source data and as measurement.

So, we first find all gps readings from real traces and "sessionise" them. This effectively boils down to breaking the data from the same device into sessions whenever:
1. There is an explicit `StartSession` message
2. There is a gap of N minutes between successive readings (N = 10 minutes probably good enough)

Then, we look at any gps readings in each session that come within M metres of a known water crossing. This is where it is probably a good idea to first normalise a session to include a version of the path that has a CRS in metres (a projected CRS). Same goes for the water crossings dataset. For simplicity, since we are covering Germany, ideally it'd be good to use a single CRS for now.

Once we have some gps readings for each water crossing for each session, we minimise this to just a single example for each water crossing per trace, using the closest match. This should give us a set of water crossings per session. We treat this as our ground truth.

We then implement a simple predictor which functions something like:
1. Receive latest GPS reading
2. Find all water crossings within D distance (in metres); remember this for later
3. If we have a previous set of water crossings:
    * find overlap between sets, and compare distances for each pair of old and new, and work out distance delta (delta = new - old)
    * for those where delta is negative (we've gotten closer) calculate velocity
    * emit prediction of wall-clock time we will pass each water crossing based on current distance to water crossing and current velocity towards it

We can run this predictor for each gps reading in each session, and then assess as follows:
* precision = for each water crossing, whenever we predicted that we would cross at time T_P, what was the actual T_A, and was it within some tolerance e.g. 30 seconds. count each of these as a boolean yes/no
* recall = for each water crossing that was ultimately passed in a session, did we make a prediction for it?

This measurement framework and predictor can both likely be improved, but we need to start with something.

## Refactor to Medallion Architecture

I think at this point we need to cleanly separate our bits of data processing and storage into a [medallion architecture](https://motherduck.com/glossary/medallion-architecture/). In this context this means something like:
* bronze:
    * raw gps and accel sensor readings, recorded live in redis and extracted via `recorder`
    * motis train samples, recorded via `motis_poll`
    * point in time extracts from OvertureMaps restricted to our needs e.g. rail/water for Germany
* silver:
    * gps readings sessionised and normalised into standard geometries
    * derived water crossings, represented as an enriched OvertureMaps segments and connector dataset extended/restricted to only what we need
* gold:
    * results of runs evaluating particular predictor versions against silver datasets

### Tasks

...

## ...

### Tasks 

...

## Slice: Spikes on Device Support

### Target

Ultimately I'd like to be able to run the live predictor as an app installed on my ["M5StickC PLUS2"](https://shop.m5stack.com/products/m5stickc-plus2-esp32-mini-iot-development-kit?variant=44269818216705) with a ["GPS/BDS Unit v1.1 (AT6668)"](https://shop.m5stack.com/products/gps-bds-unit-v1-1-at6668?variant=45727253692673).

I'd like to use a series of Spikes to show this is possible by incrementally building a small app that can show current time + GPS reading (lat, lon) on the screen and exposed over BLE.

### Notes & Gotchas (hardware realities)

- **Toolchain = Xtensa `std` path.** ESP32-PICO-V3-02 is Xtensa: install the fork via `espup`, scaffold from `esp-idf-template` (target `xtensa-esp32-espidf`), flash + log with `espflash flash --monitor`. Confirms the "std not no_std" call.
- **De-risk Crux first.** Before Spike 2, confirm `crux_core` compiles for `xtensa-esp32-espidf`. It's only built/tested against std targets (WASM/iOS/Android); cheap to learn now if it doesn't.
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

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. This allows a recommendation to be given about which direction to look relative to the train seat.

## Slice: Adding POI's from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POI's by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to to identify exactly what the POI is based on the image.