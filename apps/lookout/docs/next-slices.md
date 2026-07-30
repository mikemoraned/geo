# Next Slices

## Slice: Spike on device support for distance lookup

### Target

Ultimately we are going to need to be able to do distance lookups on an M5 device between a current GPS-derived lat/lon and some sort of database of static lat/lon points. This slice is about doing one or more spikes which show this is possible given the size/speed constraints of the device. 

This should be based on a dataset of water crossings in silver which is transformed to something in gold we can then load onto a device.

### Technical approach (from design discussion)

**Brute force, no index.** One GPS fix at ~1 Hz against a static point set. Even 50k points (~400 KB) scans in single-digit ms at 240 MHz — well inside the budget. A k-d tree isn't worth it at this scale.

**Single global CRS, unprojected — WGS84 lat/lon (EPSG:4326).** Keep points in lat/lon; no on-device projection, no zone limits, works anywhere. Distance is **haversine**, computed in **f32** (the ESP32 FPU is single-precision; f64 is software-emulated). At 1 Hz brute force the cost is irrelevant, so we take the exact great-circle distance and skip approximations. Output is metres, straight into both consumers below. (The predictor's projected metre-CRS stays an offline/evaluation concern; the device just emits metre distances from lat/lon.)

**Store lat/lon as f32 (or i32 1e-7°).** Raw f32 degrees give ~0.5–2.5 m resolution, under GPS noise; use i32 at 1e-7° (~1 cm, fits i32) if you want more.

**Flat packed buffer, built in gold — no on-device Arrow or rkyv.** Parallel columns `lat[]`/`lon[]`/`id[]`; on device (std) load via `std::fs` or `include_bytes!` and zero-copy cast with `bytemuck`/`zerocopy` (mind 4-byte alignment).

**One scan, two consumers:** display top-N-closest and the predictor's "within D metres" come from the same pass.

**Lives in the Crux core** — pure compute, no effects, so untouched by the `crux_core =0.16.2` pin.

#### Rejected / deferred

- **PMTiles** — a map-tile rendering format, not a query index. Only relevant if we later draw a map.
- **k-d tree (kiddo)** — deferred; only worth it at far larger counts/rates. If needed: precompute 3D unit-vectors (keeps it global, no projection) and ship the tree offline via rkyv.
- **arrow-rs on device** — the columnar *layout* is good, the *crate* is heavy analytical machinery. Arrow/Parquet on the gold side only.

#### Open questions

- Actual size of the crossings set (sets scan-time headroom; approach holds regardless).

### Straw Man

As part of Spikes on Device Support (see completed-slices.md) we have apps/lookout/spikes/m5/spike3-gnss. We should base a spike5+ in this which:
* Can load and display everything spike3 can + show distance from a random set of lat/lon points. We should show the top N closest from current position, such that all N fit on screen, alongside the distance (in metres or km) to each
* Can do same, but based on a dataset of real water crossings

### Tasks

...

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

## Slice: Deploy predictor on M5 device

### Target

Run the real predictor on the M5StickC PLUS2, fed by its own GPS unit rather than by replayed
traces — the point the device spikes were building towards.

### Constraint inherited from the device spikes: `crux_core` is pinned

**`crux_core` must stay pinned at `=0.16.2` on device.** On 0.19, Crux + BLE reboots the
M5 every 4 seconds to 7 minutes, always inside crux's per-effect `Command`/crossbeam machinery.
Pinning 0.16.2 fixes it — 30 minutes stable with a client connected and a real fix — but the
cause was never identified, only avoided; it is some change between 0.17 and 0.19. Stack
overflow, both task stacks, heap exhaustion, fragmentation, heap overrun, PSRAM, allocation
volume and model placement were all ruled out by measurement. Evidence and the four wrong
diagnoses are in `apps/lookout/spikes/m5/spike4-ble/README.md`.

Two consequences for this slice:

- The predictor core has to compile against 0.16.2's API (one extra associated type,
  `type Capabilities = ()`), so don't adopt newer crux features in the shared core.
- If a newer crux becomes necessary, the work is bisecting 0.17/0.18 to find the change (one
  flash and a 15-minute soak each), or dropping crux from the device shell — which would
  forfeit the shared-core argument that justified Crux in the first place.

### What the spikes already established

The spikes leave a working skeleton to hang the predictor on: `apps/lookout/spikes/m5`, with
a Crux core split from an esp-idf shell so the core stays testable on the laptop. The
predictor should *be* that core. See `.claude/memory/m5-esp32-toolchain.md` for the board's
gotchas (power hold, panel offset, RX pin, stack size, UART buffer).

### Notes & Gotchas (what the on-device GNSS actually behaves like)

Measured on the GPS/BDS unit (AT6668) with the receiver deliberately held still:

- **Good geometry** (8 satellites, HDOP 2.4): position wanders ~1.8m horizontally over 12s,
  and differencing consecutive one-second fixes implies ~1.0 m/s of motion. The receiver's
  own Doppler-derived speed is far more honest at 0.04–0.91 knots (≤0.5 m/s).
- **Poor geometry** (6 satellites, HDOP 4.4): position wanders ~4.5 m/s, *and* the reported
  speed claims 4.13 knots. In poor conditions the Doppler speed lies too.

So a predictor that derives velocity by differencing distance-to-crossing between successive
readings — as the minimal predictor's straw man does — will be dominated by noise at these
scales, and a stationary device can appear to be closing on a crossing fast enough to emit
confident nonsense. Two responses, both wanted here:

1. Prefer the receiver's reported speed over position-differencing, and/or difference over a
   longer baseline than one reading.
2. **Gate on fix quality.** HDOP and satellite count are the difference between ~0.2 m/s and
   ~4.5 m/s of phantom motion, so the on-device predictor needs them as inputs, not just
   lat/lon. `RMC`/`GGA` carry both.

A train at speed should swamp this noise, but approach and departure — exactly when a crossing
prediction is being refined — are the low-speed regime where it bites. Worth checking whether
the same holds for the phone traces before assuming it is device-specific.

Also: the receiver reports true UTC in `ZDA`/`RMC` before it has any position fix, so
wall-clock time needs neither NTP nor the BM8563 RTC.

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. This allows a recommendation to be given about which direction to look relative to the train seat.

## Slice: Adding POI's from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POI's by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to to identify exactly what the POI is based on the image.