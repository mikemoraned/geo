# crossings

Turns the silver water-crossings dataset into the flat point buffer the M5 device scans.

```sh
just pack-crossings                                   # defaults, from the repo root's apps/lookout
just pack-crossings --input <parquet> --output <file>
just pack-crossings --bbox 13.0,50.9,14.5,51.9        # west,south,east,north
```

The device holds every crossing in RAM and brute-force scans the lot against each GPS fix, so
what it needs is not a queryable dataset but a packed array of coordinates. At the measured
size — 5,749 crossings for Germany — that is **69,000 bytes**, small enough to `include_bytes!`
into the firmware, and small enough that an index would be a waste of the effort.

## The `.pointset` layout

Little-endian throughout. Both the ESP32 (Xtensa) and the machine that builds the file are
little-endian, and the device casts these bytes in place rather than decoding them, so the
file's byte order *is* the device's.

```
offset  bytes  field
     0      4  magic "XING"
     4      4  version, u32 (currently 1)
     8      4  count, u32 — the number of points, n
    12     4n  latitude,  [f32; n]   degrees, WGS84
 12+ 4n     4n  longitude, [f32; n]   degrees, WGS84
 12+ 8n     4n  id,        [u32; n]
```

Total is `12 + 12n` bytes. The header is 12 bytes — a multiple of 4 — so every column starts
on a 4-byte boundary and can be cast in place without a shuffle.

Three parallel columns rather than an array of structs: a scan reads latitude and longitude
and touches the ids only for the handful of points it ends up reporting, so the ids stay out
of the way of the pass that has to be fast.

### What a reader must check

`pointset::unpack` in this crate is one implementation; the device core is the other, and they
have to agree. A reader has to reject:

- fewer than 12 bytes — there is no header to read
- a magic that is not `XING` — some other file entirely
- a version it does not know, rather than reading an unfamiliar layout as if it were familiar
- a length that is not exactly `12 + 12n` for the `n` in the header — a truncated file would
  otherwise read as points made of whatever bytes happened to follow

### Alignment, on the device

`include_bytes!` yields a `&[u8; N]` with **alignment 1**, so casting it straight to `&[f32]`
is unsound and will fault on Xtensa. Put the bytes behind a type that forces the alignment:

```rust
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

static POINTS: &Aligned<[u8]> = &Aligned(*include_bytes!("crossings.pointset"));
```

## Coordinates are `f32`

`f32` degrees resolve to **≤0.21 m** over the German crossings (mean 0.11 m) — far under what
the receiver resolves, and under the metre-scale wander a stationary fix shows even in good
conditions. It also suits the ESP32's single-precision FPU, where `f64` is emulated in
software. `i32` at 1e-7° was the alternative, at ~1 cm; the extra precision buys nothing
against a GPS error budget measured in metres.

## Ids name a crossing, not a row

`id` is the low 4 bytes of the md5 of `(rail_id, water_id, frac)` — what the crossing *is*,
not where its row landed. So an id survives a rebuild of the dataset, a `--bbox` that keeps
only part of it, and any reordering, which means a prediction made on the device and a ground
truth derived on the laptop can name the same crossing.

`frac` — how far along the rail segment the crossing sits — is part of the key because
`(rail_id, water_id)` is **not** unique: a meandering river meets a single segment up to 13
times in this dataset.

Four bytes is few enough that two distinct crossings can collide by chance (~0.4% over 5,749
points). A pack run detects that and fails rather than shipping one name for two crossings.
The real dataset is clean: 5,749 crossings, 5,749 distinct ids.

## Points are written in id order

The same crossings therefore pack to the same bytes whatever order the source happened to
store them in, so a rebuild that only reorders rows produces an identical file and needs no
reflash.

## Input

Currently the water-crossings notebook's export (`data/water/<version>/crossing_reps.parquet`).
Position is read from the WKB `geometry` column and never from the `lat`/`lon` columns that
export also carries — the medallion silver `water_crossing` dataset this will move to keeps
position only in the geometry, so depending on those columns would break on arrival.
