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
- [x] Decide what `id` is (dense u32 index into a side table vs. a hash of the Overture id) — the device only needs enough to name a crossing on screen — **a stable u32: the low 4 bytes of md5 over `(rail_id, water_id, frac)`**. Chosen over a dense index so an id survives a rebuild, a bbox restriction and a reordering, and so a prediction made on device and a ground truth derived on the laptop name the same crossing. Two findings forced the key: **`(rail_id, water_id)` is not unique** (5,275 pairs for 5,749 rows — a meandering river meets one segment up to 13 times), so `frac` is part of the identity; and `frac` is hashed as `to_bits()`, never as formatted text. u32 makes chance collisions possible (~0.4% at this size), so `id::assign` refuses rather than shipping one name for two crossings — pinned by a real colliding pair found by search. **The real set is collision-free: 5,749 crossings → 5,749 distinct ids.**
- [ ] Decide what the device shows *beside* a distance, given **the export has no name column at all** — no "River Elbe" is available. The only labelling material is `water_class` (16 values) / `water_subtype` (9), which would fit a `u8` code in the buffer; the alternative is carrying names from Overture, which the crossings pipeline currently drops
- [ ] Write the gold buffer: a small header (magic, version, count) followed by parallel `lat[]`/`lon[]`/`id[]` columns, 4-byte aligned. Pick f32 degrees vs i32 1e-7° and say which, with the resolution argument
- [ ] Round-trip test the writer against a reader in the same crate (write known points, read them back, assert coordinates survive within the representation's resolution) — the device reader in spike 5 is the second implementation of the same format
- [ ] Document the byte layout in a README beside the packer: it and the device reader are the only two things that know the format

**Spike 5 — distance lookup, random points**
- [ ] Scaffold `spikes/m5/spike5-distance` from spike3-gnss: core/shell split, its own `Justfile` (`test` / `flash`), `crux_core` pinned `=0.16.2`
- [ ] In the core, a type wrapping the packed buffer that validates the header and zero-copy casts the columns with `bytemuck`/`zerocopy` — a bad or misaligned buffer is a `Result`, not a panic. Note whether the format constants can be shared with the packer crate or have to be duplicated (the spike core is its own workspace member, esp-free but also parquet-free)
- [ ] TDD the scan: stub returning nothing → tests → implement. One brute-force pass in f32 haversine over the buffer, returning both the top-N nearest (metres) and everything within D metres. Cover known great-circle distances, fewer points than N, an empty set, and that both consumers come from the same pass
- [ ] Generate a seeded random point set at the measured size, embed with `include_bytes!`
- [ ] Extend the view model and shell rendering: spike3's clock + lat/lon, plus the top-N nearest with distance, N chosen so it all fits the 135×240 panel
- [ ] Flash and confirm with a real fix; log per-fix scan time in µs and record it in the README against the single-digit-ms prediction

**Spike 6 — real water crossings**
- [ ] Run the packer over the real crossings and get the buffer onto the device — `include_bytes!` vs SPIFFS/`std::fs`; decide on size grounds and record why
- [ ] Show the top-N nearest real crossings with distances; cross-check a handful against distances computed in the notebook for the same lat/lon (f32 tolerance)
- [ ] Field-check on a real route: the nearest set should change sensibly as position moves, and fix quality (HDOP/sats) should visibly bound how much the distances jitter
- [ ] Record in the README: buffer size, flash/heap footprint, scan time, and whether the chosen lat/lon representation held up

**Conclusion**
- [ ] State whether brute-force-with-no-index holds at the measured size, and how much headroom there is before a k-d tree earns its place
- [ ] Confirm the core still builds and tests on the laptop against `crux_core =0.16.2`, so the predictor slice can adopt it unchanged
