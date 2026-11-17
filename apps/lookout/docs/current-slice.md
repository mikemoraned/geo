# Current Slice: Show map context within App itself

### Target

* show the train lines in current region of display: this shows which trainlines are nearby and so which are we likely to be on
* show the moving window of last N samples seen as a "Snake" style: this provides context of recent movements

### Straw Man

Building on what we did in in "Slice: Spike on Maps using Kiosk display as an example", we'd add ability of the App to request map data for a region and then return that to the View.

The lookup would be done by something embedded in Web Shell which can query protomaps or similar (whatever we decided to use based on the Spike). 

This lookup is optional in that on the M5 it likely won't have access to map data live and so would have to be embedded alongside. This is of limited value right now as the M5 has a very limited display and so can't really show a Map. Later on we may use devices that have a better display but for now it adds nothing.

The display can be kept quite dumb and just given a list of lines to display, named by category. Coloring can then be decided by category. For example, the View can return a line with a category of "trace" which is the recent positions of the device from GPS, and separately a set of lines with a category of "trainline" which are the local train lines we received.

### Decisions

Decisions taken before starting, as each changes what gets built. Confirmed 2026-11-16.

- **The rail comes from the map's `rail` layer as it stands.** That layer holds only the rail
  segments a water crossing names, so rail away from water stays undrawn. Reading the layer
  `map.pmtiles` already serves costs no new data and no growth in the committed file. Every
  non-tram rail segment in each extract would fill the gaps, but DE's rail alone is 189 MB of
  parquet.
- **The core asks for lines, and the shell answers.** A new effect carries the area the core
  wants rail for, and a new event brings back the area and its lines. The shell reads them, as it
  reads the crossings, so the core never learns where the lines come from.
- **Lines cross the bridge as WKB geometries in longitude and latitude, base64 inside the JSON,
  both ways.** The shell hands the core WKB, and the view hands the display WKB, so neither side
  sees a tile, its coordinates, or PMTiles. WKB is the OGC's standard binary form, at 16 bytes a
  vertex before base64. GeoJSON and WKT spell each coordinate out as text, which is verbose.
  Google's encoded polyline is smaller, but it holds a line with no geometry type.
- **A shell says whether it draws lines, as it says whether it carries crossings.** The device
  draws no map, so its core never asks. The alternative, a request the device answers with
  nothing, sends an effect per fix to a shell that discards it.
- **The area is a box around the fix, wider than the predictor's radius.** The core asks again
  once a fix nears the box's edge, and drops lines that arrive for a box it has left. Asking on
  every fix would read tiles at the fix rate.
- **The snake is the fixes within a time window before the latest.** A window means the same
  stretch of track at the phone's fix rate and at a kiosk replay's, which skips samples. A count
  of N samples covers a different distance on each page.
- **The view gives the display lines named by category, `rail` and `trace`.** The element picks
  a colour per category, draws the lines under the crossings, and clips them to the outer ring.
  It projects each vertex as it projects a crossing, so a line bends under the symmetric-log
  scale.
- **Every page with the element draws the lines.** `/live` and `/kiosk` both get them, and the
  kiosk's MapLibre map stays as it is.
- **The element reads the rail at zoom 12, the `rail` layer's highest.** A tile there spans
  about 6 km at these latitudes, so a box twice the 5 km radius covers a few tiles.

#### Rejected / deferred

- **Every rail segment in each extract.** It comes back if the gaps away from water hide the
  line a journey runs on. Its size by maximum zoom decides whether it joins `map.pmtiles` or
  takes a file of its own.
- **Lines on the device.** A device with a display that can show a map brings them back. The
  device would then carry its rail in flash, as it carries its crossings.
- **A MapLibre map inside the element.** The display stays dumb, and the core decides what it
  shows.

#### Open questions

- **How long the snake's window is, and how wide the box is.** A kiosk replay settles both. The
  window decides how far the trail reaches past the outer ring at speed.
- **Whether reading a box's tiles keeps up on a phone.** If it lags, the box grows so the core
  asks less often.

### Tasks

#### Refactors / extensions

- [x] Find what `decodeTile` from `maplibre-mlt-1.3.0.js` gives as a feature's geometry: its
  coordinates, its extent, and how it splits a line. Do this first, since the element's reading
  depends on it.
  - A feature's `geometry.type` is a `GEOMETRY_TYPE` number, 1 for a line and 4 for several.
    `geometry.coordinates` holds one array of `{x, y}` points per line, in integer units of the
    table's `extent`, 4096. A line is cut at the tile's edge and runs on into the buffer, so `x`
    and `y` go below 0 and above 4096.
- [x] Move `layerFeatures` and `tilesCovering` from `static/kiosk/journeys.js` into
  `static/tiles.js`, and import them from there in `journeys.js`.
- [x] Choose a JavaScript library that writes and reads WKB, and vendor it under `static/vendor/`.
- [x] Add a reader of a layer's lines in `static/tiles.js`, returning WKB geometries in longitude
  and latitude.

#### The core asks for rail and keeps a trace (`platform-core`)

- [x] Check that georust's `wkb` crate builds for the device's target, since `platform-core`
  builds there. If it fails, decide where the WKB decoding lives before the next task.
- [x] Add a `Shell` function saying whether the shell draws lines, false for `Device`.
- [x] Add the `Lines` effect, carrying the box, and the `Lines` event, carrying the box and its
  lines as WKB geometries, in `app.rs`.
- [x] Keep the box and its rail in `Model`, ask for a new box once a fix nears the edge, and drop
  lines for a box the core has left. `Reset` clears both.
- [x] Keep the fixes within the snake's window in `Model`, pruned as each fix arrives.
- [x] Test each in `app.rs`: the first fix asks, a fix inside the box does not, a fix near its
  edge does, stale lines are dropped, and a `Reset` forgets.

#### The browser draws them (`web` core, bridge, and element)

- [x] Add `lines` to `web_core::ViewModel`, each a WKB geometry with its category, and draw lines
  from `Browser`. Leave the colours to the element.
- [x] Update the bridge's JSON contract test in `bridge/src/lib.rs` for the new effect, event,
  and view.
- [x] Answer the `Lines` effect in `lookout-predictor.js` by reading the `rail` layer of
  `/map.pmtiles` through `static/tiles.js`.
- [x] Draw each line in `lookout-predictor.js` under the crossings, coloured by category and
  clipped to the outer ring.
- [ ] Rebuild the wasm with `just wasm`, then check `/live` and `/kiosk` in a browser.
- [ ] Choose the snake's window and the box's width from a kiosk replay, only after the element
  draws both.

#### Wrap-up

- [ ] Describe the lines in `crates/platform-core/README.md`: who asks, when, and what a shell
  answers.
- [ ] Describe the lines in `crates/platform/web/README.md` and in the picture in `docs/web.md`.
- [ ] Name the element as a reader of the `rail` layer in `crates/map/README.md`.
- [ ] Run `just test-no-docker` and `cargo clippy --workspace`, and ask the user to run
  `just test`.
