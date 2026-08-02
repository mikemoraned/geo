# `Lookout` Motivation

I travel on trains a lot, and things I would have liked to photograph go past before I see them coming — most often rivers and bridges. This app is to remind me when something interesting is coming up.

# Straw-man

Many things count as interesting; we start with rivers and bridges. A point of interest is where a train line crosses a river or a larger body of water, and we notify when it is a minute away at the current rate of travel.

What we discover may change this, but it is the approach for now.

## Architecture

We follow the [ports and adapters pattern](https://8thlight.com/insights/a-color-coded-guide-to-ports-and-adapters), and in particular the different types of things we have to interface with are managed by this pattern:
- sensors e.g. GPS and accelerometers
- geo datasets
- ui's, on different platforms, including actions such as notifying the user
- persistence of derivations of state

## Approach

This largely breaks down into doing this live:
1. Using the assumption that they are currently on a train, identify which trainline they are currently on, based on how fast they are travelling and where they are
    - this will involve taking in absolute position sensors (i.e. GPS) and also relative ones like accelerometers
    - we can deal with uncertainty in position by clamping to the nearest trainline
    - we can use some more advanced modelling based on past position etc
2. Find next poi of interest that is on that line
3. Predict time of arrival at that poi and alert if this is less than a minute

This means that ahead of time we have to build a dataset that supports the lookup:
1. Get the train network for an area from [OvertureMaps](https://docs.overturemaps.org/guides/transportation/)
2. distill this down into a series of segments, or whatever supports the lookup

Accelerometers from coupled devices — my laptop, my phone, and my iPad — can improve accuracy further, which is what a local device comms library is for: it shares their readings across them. An [M5](https://m5stack.com) device could serve as a dumb accelerometer, providing sensor data without running the whole stack.

# Constraints, Trade-offs, and Technology Choices

- Use the [crux](https://redbadger.github.io/crux/) library for ports and adapters
- Use the [iroh](https://docs.iroh.computer/quickstart) library for multi-device comms
- data is persisted in the layered store described in [medallion.md](medallion.md): parquet
  from bronze onwards, with sqlite only as a landing/external format for live capture
- sensor data is visualised in https://rerun.io
- all code inside the centre of the architecture should be in Rust i.e. all business logic is in Rust
- for front-end on web we should follow a single-page-app pattern and use typescript + https://www.solidjs.com

# Learnings

## Constraints

- **The dev laptop cannot self-source motion data.** The dev machine is an Apple Silicon
  (M3) MacBook Air (`Mac15,12`); Apple Silicon dropped the Sudden Motion Sensor, so there
  is no built-in accelerometer the OS or browser can read — `DeviceMotionEvent` never
  fires in desktop Safari/Chrome, and there is no SMC motion sensor. A "browser + server
  both on the laptop, localhost only" setup can validate the websocket transport and
  persistence plumbing, but **cannot** produce real accelerometer data. Any slice needing
  real motion must use an external source:
    - **AirPods** (Pro / 3rd-gen / Max) via macOS `CMHeadphoneMotionManager` — real
      accel+gyro, wireless, no cert; a small native (Swift) helper forwards it. (Head
      motion, not device motion.)
    - **A game controller with an IMU** (PS5 DualSense, DS4, Switch Joy-Con/Pro) — real
      3-axis accel+gyro, but the browser Gamepad API does *not* expose the IMU, so it
      needs a native HID reader → websocket.
    - **iPhone/iPad running the page** — the real target, gives proper `DeviceMotionEvent`
      data, but on a LAN IP (not `localhost`) the sensor API needs a secure context, so it
      requires HTTPS via a cert (`mkcert`) or a tunnel (`cloudflared`/`ngrok`).
    - An **M5 / dedicated sensor board** remains a future option.

## Finding water crossings (Overture rail × water)

- **The hard part is deduplication, and it's water-side + spatial, not rail-side.** A single
  physical crossing appears many times because Overture (a) splits a rail line into many short
  segments, (b) stores the same river as *both* an areal polygon and a centreline, and (c) has
  parallel / multi-track rail. The intuitive fix — line-merging contiguous rail *before*
  intersecting — barely helps, because the duplication is dominated by the water representation and
  by nearby-but-distinct water bodies, not by rail fragmentation. What works: group crossing
  segments into physical tracks via shared Overture **connector ids**, then merge within each
  `(track, water body)` by distance. This keeps genuinely distinct crossings — parallel tracks, or
  a river that horseshoes back over the *same* track — separate, which plain distance clustering
  (DBSCAN) cannot.

- **A 2D rail∩water intersection is not "water you can see from the train."** It includes rail
  running *under* water in a tunnel (real false positives near Hamburg) and abandoned/disused track
  where no train runs. Overture segments carry `rail_flags` with fractional `between` ranges tagging
  `is_tunnel` / `is_covered` / `is_abandoned` / `is_disused` / `is_under_construction` (and
  `is_bridge` — the opposite, keep it). Locating each crossing as a fraction along its segment
  (`ST_LineLocatePoint`) lets you test membership in those ranges and drop the invisible ones. That
  same fraction is the Overture **Connector** position, so the visibility filter and the
  connector-mapping idea are one mechanism.

- **Overture representation quirks to expect downstream:** water is modelled redundantly (polygon +
  centreline of the same river); rail is fragmented into short segments whose **connector ids**
  reconstruct a physical track; water polygons can have island holes that split one span into
  several channels. Any "how many times does this line cross water" logic must account for these.

- **DuckDB `spatial` reading Overture GeoParquet directly is enough** for country-scale extraction
  and intersection (bbox-pruned, in SQL) — no heavier engine needed here — and a local Overture
  mirror is far faster than S3. A small library of **bbox test cases with expected counts**, plus a
  viewer linking to the OvertureMaps explorer, repeatedly caught wrong assumptions (including a
  hand-set expected count that was itself wrong).
