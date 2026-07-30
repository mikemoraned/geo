# Current Slice: Spike on device support for distance lookup

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

**Gold: the packer CLI**

A Rust CLI that reads the silver crossings GeoParquet and writes the gold device buffer —
following the `transport`/`enrich` shape (a crate with `src/bin/`, clap args, a `just` recipe),
and keeping to the "Rust derives, Python reads" convention.

**Input is the notebook's `data/water/<vN>/crossing_reps.parquet`, not the medallion silver
dataset.** The `lookout-slice-minimal-live` branch is well ahead of this one and has moved
crossings into a medallion silver `water_crossing` dataset (`crates/model/src/crossing.rs`),
but that dataset is defined and not yet written. This slice's question is device feasibility,
not pipeline shape, so it takes the file that exists today; switching the packer's reader when
medallion lands is a contained change. Note `CrossingId` there is a string, which the device
buffer cannot hold — the id-representation task has to survive that move.

- [x] Measure the actual crossings set: count and bbox of `data/water/v7/crossing_reps.parquet`, so the scan-time headroom is a measured number rather than the "even 50k points" assumption above — measured on **v8**, since no v7 output exists and v8 is the current notebook (on the `lookout-slice-minimal-live` branch). **5,749 crossings**, all Points in EPSG:4326, bbox lon 6.08–15.03 / lat 47.43–54.92 (Germany). That is ~9× smaller than the 50k the approach was sized against: **67 KB at 12 bytes/point**, so the buffer is `include_bytes!`-sized and brute force is settled. The parquet already carries plain `lat`/`lon` float columns beside the WKB geometry. f32 degrees round-trip to **≤0.21 m** over this set (well under GPS noise), so f32 wins and i32 1e-7° isn't needed.
- [x] Scaffold the crate and `src/bin/` packer with clap args for input parquet, output path, and an optional bbox filter; add a `just` recipe for it — `crates/crossings` + `src/bin/pack_crossings.rs` + `just pack-crossings`. The bbox is a `Bbox` newtype parsed via `FromStr`, so clap rejects a bad window rather than the packer discovering it later. Output defaults to `data/gold/crossings.pointset`, gitignored as regenerable. **Also fixed a pre-existing break:** `spikes/m5/spike3-gnss/core` was listed in the app workspace's `members` *and* declares its own `[workspace]`, which left cargo with two roots for one directory — every `cargo` command in `apps/lookout` failed, including `just test`. Spike cores stay out of the members list; the spike's own `just test` runs it.
- [x] Read the silver GeoParquet and project it to (lat, lon, id) — decoding the WKB point geometry the notebook writes, and failing with a `thiserror` variant on anything that isn't a point — `silver::read` over `parquet` + `wkb`, no engine in front of it. Reads position **only** from the geometry, never the `lat`/`lon` columns the notebook also writes: the medallion `WaterCrossingRow` has no such columns, so depending on them would break on the dataset this moves to. Columns are `cast` before downcasting, so `large_string`/`binary` widths don't matter. Verified against the real file: 5,749 crossings, and the bbox filter agrees with the measured extent
- [x] Decide what `id` is (dense u32 index into a side table vs. a hash of the Overture id) — the device only needs enough to name a crossing on screen — **a stable u32: the low 4 bytes of md5 over `(rail_id, water_id, frac)`**. Chosen over a dense index so an id survives a rebuild, a bbox restriction and a reordering, and so a prediction made on device and a ground truth derived on the laptop name the same crossing. Two findings forced the key: **`(rail_id, water_id)` is not unique** (5,275 pairs for 5,749 rows — a meandering river meets one segment up to 13 times), so `frac` is part of the identity; and `frac` is hashed as `to_bits()`, never as formatted text. u32 makes chance collisions possible (~0.4% at this size), so `id::assign` refuses rather than shipping one name for two crossings — pinned by a real colliding pair found by search. **The real set is collision-free: 5,749 crossings → 5,749 distinct ids.** The id is also what the device shows beside each distance — the export has no name column, so there is no "River Elbe" available, and none is wanted: the id tells two crossings apart on screen and looks one up afterwards.
- [x] Write the gold buffer: a small header (magic, version, count) followed by parallel `lat[]`/`lon[]`/`id[]` columns, 4-byte aligned. Pick f32 degrees vs i32 1e-7° and say which, with the resolution argument — `pointset::pack`. 12-byte header (`XING`, version u32, count u32), itself a multiple of 4 so every column starts aligned; little-endian, which is both the build host's and the ESP32's. **f32 degrees**, on the measured ≤0.21 m: under what the receiver resolves, and what the single-precision FPU wants. Points are written **in id order**, so the same crossings pack to identical bytes however the source ordered them — a rebuild that only reorders rows needs no reflash. Real run: **69,000 bytes** for 5,749 crossings, byte-identical across runs
- [x] Round-trip test the writer against a reader in the same crate (write known points, read them back, assert coordinates survive within the representation's resolution) — the device reader in spike 5 is the second implementation of the same format — `pointset::unpack` reads field by field via `from_le_bytes` rather than casting, so a round trip checks the *layout* rather than the host's memory representation. Rejects: no header, wrong magic, unknown version, and a length that doesn't match the claimed count
- [x] Document the byte layout in a README beside the packer: it and the device reader are the only two things that know the format — `crates/crossings/README.md`, including what a reader must reject and the `#[repr(C, align(4))]` wrapper the device needs, since `include_bytes!` yields alignment 1 and casting that to `&[f32]` faults on Xtensa. The wrapper is compiled and checked, not just quoted

**Spike 5 — distance lookup, random points**
- [x] Scaffold `spikes/m5/spike5-distance` from spike3-gnss: core/shell split, its own `Justfile` (`test` / `flash`), `crux_core` pinned `=0.16.2` — copied from spike 3 and ported to the 0.16.2 API (`type Capabilities = ()`, a `_caps` argument to `update`), which spike 4 had already worked out. Pinned even though this spike has no BLE and so cannot hit the reboot: the predictor this core leads to does, and finding out later would mean bringing the core back down. Both crates carry their own `[workspace]` and lock, out of the app workspace. `just test`: 12 tests green on 0.16.2
- [x] In the core, a type wrapping the packed buffer that validates the header and zero-copy casts the columns with `bytemuck`/`zerocopy` — a bad or misaligned buffer is a `Result`, not a panic. Note whether the format constants can be shared with the packer crate or have to be duplicated (the spike core is its own workspace member, esp-free but also parquet-free) — `pointset::PointSet` borrows the columns via `bytemuck::try_cast_slice`, so nothing is copied into RAM; refuses no-header, wrong magic, unknown version, wrong length, and **misalignment**, which is the `include_bytes!` trap (alignment 1) and would otherwise fault on Xtensa. **The constants are duplicated, not shared:** `crossings` pulls arrow/parquet/clap and could never build for this target, and a path dependency would also break the convention that an archived spike still builds years later. What makes the duplication safe is `tests/four-crossings.pointset` — a **real file from the packer**, committed, asserted against the exact `f32` values the dataset's geometry holds. If the two implementations drift, that test stops reading
- [x] TDD the scan: stub returning nothing → tests → implement. One brute-force pass in f32 haversine over the buffer, returning both the top-N nearest (metres) and everything within D metres. Cover known great-circle distances, fewer points than N, an empty set, and that both consumers come from the same pass — `scan::nearby`, 13 tests: stub first (11 of 12 red), then implement. **Haversine comes from georust** (`geo`, generic over `CoordFloat`, so f32 goes straight through) rather than hand-rolled — every dedicated haversine crate is f64, which would defeat the single-precision FPU argument. `default-features = false` drops earcutr/spade/**rayon**, 39 transitive crates down to 22, and **it compiles for `xtensa-esp32-espidf`** — verified by building the shell, which was the open risk in taking the dependency. Known distances are checked against an *independent* haversine (a degree of latitude = 111,195 m ±10), not against geo itself. The top-N is kept ordered as points arrive rather than sorting the whole set, which at 5,749 points would be 69 KB of heap per fix
- [x] Generate a seeded random point set at the measured size, embed with `include_bytes!` — `random_crossings` bin + `just random-crossings`, sharing the packer's own format code so the made-up set is the same kind of file as the real one. 5,749 points over the real extent, from `ChaCha8Rng` — **not `StdRng`, which explicitly does not promise the same stream across versions**; the committed file has to stay the file the device was flashed with. Byte-identical across runs. Embedded in the core as `carried::crossings()` behind a `#[repr(C, align(4))]` wrapper, with the length written out so a regenerated file of a different size stops compiling rather than failing quietly. Builds for xtensa with the 69 KB in flash
- [x] Extend the view model and shell rendering: spike3's clock + lat/lon, plus the top-N nearest with distance, N chosen so it all fits the 135×240 panel — **N = 5**. `FONT_10X20` on a 135-pixel panel gives **13 characters a line**, which sets the format: 6 hex digits of the id, a space, and a distance in at most 6 (`942m`, `1.5km`, `250km`, `>999km` past a thousand). Tests assert no distance in any band can overflow a line. Five 22-pixel lines from y=130 end at 218, clear of 240. Also shows the **`within` count** beside the fix, so both halves of the one pass are on screen and cannot disagree. The scan runs when the **position** changes, not per sentence — a dozen arrive a second carrying the same fix, and 5,749 haversines each would be a waste of the second. Lines are padded to 13 characters because the panel has no erase, so a shortening list would otherwise leave its tail behind
- [x] Flash and confirm with a real fix; log per-fix scan time in µs and record it in the README against the single-digit-ms prediction — **7,835–8,377 µs** for 5,749 crossings on a real fix, i.e. **~1.4 µs per crossing** (~330 cycles at 240 MHz) and **0.8% of a one-second budget**. The prediction of single-digit ms held — but **its reasoning was an order of magnitude out**: it argued "even 50k points scans in single-digit ms", and at the measured rate 50k would take ~70 ms. It survived only because the real set is 9× smaller than the figure the estimate was built on. Recorded in the spike README. Also removed spike 3's per-sentence NMEA echo, which buried the one line worth reading

**Spike 6 — real water crossings**
- [ ] Run the packer over the real crossings and get the buffer onto the device — `include_bytes!` vs SPIFFS/`std::fs`; decide on size grounds and record why
- [ ] Show the top-N nearest real crossings with distances; cross-check a handful against distances computed in the notebook for the same lat/lon (f32 tolerance)
- [ ] Field-check on a real route: the nearest set should change sensibly as position moves, and fix quality (HDOP/sats) should visibly bound how much the distances jitter
- [ ] Record in the README: buffer size, flash/heap footprint, scan time, and whether the chosen lat/lon representation held up

**Conclusion**
- [ ] State whether brute-force-with-no-index holds at the measured size, and how much headroom there is before a k-d tree earns its place
- [ ] Confirm the core still builds and tests on the laptop against `crux_core =0.16.2`, so the predictor slice can adopt it unchanged
