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

- [ ] Measure the actual crossings set: count and bbox of `data/water/v7/crossing_reps.parquet`, so the scan-time headroom is a measured number rather than the "even 50k points" assumption above
- [ ] Scaffold the crate and `src/bin/` packer with clap args for input parquet, output path, and an optional bbox filter; add a `just` recipe for it
- [ ] Read the silver GeoParquet and project it to (lat, lon, id) — decoding the WKB point geometry the notebook writes, and failing with a `thiserror` variant on anything that isn't a point
- [ ] Decide what `id` is (dense u32 index into a side table vs. a hash of the Overture id) — the device only needs enough to name a crossing on screen
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
