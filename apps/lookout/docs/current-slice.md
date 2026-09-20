# Current Slice: Embed Predictor on website

### Target

Embed the crow-flies predictor in the existing site at https://lookout-hom.fly.dev, leaving it
with four pages:

* `/record` — the recording page as it stands today, moved off the homepage and working the
  same way
* `/live` — the app, reading GPS from the browser
* `/kiosk` — the app in kiosk mode, replaying a session recorded in the past as though it were
  arriving now
* `/` — a short description of the project, linking to the other three

The app ships as a web component: a custom element owning the Crux core compiled to WASM,
which takes injected GPS and draws a canvas:

* a dot in the middle for where we are, its size proportional to our estimated speed
* the predicted crossings as small dots, placed by a hyperbolic mapping that gives nearer
  distances more of the canvas. Nothing is drawn beyond a maximum distance, so a dot on the
  rim is a crossing at that maximum.

A page then places the tag and feeds it: `/live` from the browser's geolocation, `/kiosk`
from the replayed session.

### Implementation Choices

The website stays plain: hand-written pages, no framework. The Crux app returns a view of real
lat/lon positions, with no hyperbolic mapping in it; D3 maps those onto a circular canvas in
the shell. What that settles:

- **One core, two shells.** `platform-core` is device-shaped: it eats NMEA sentences, and its
  ViewModel is pre-formatted 13-character lines for a 135-pixel screen. Prediction state moves
  into a shared core, and each shell projects its own ViewModel from it — the panel's strings
  for the device, structured lat/lon for the web. A second core over the same `predictor`
  would drift from the first.
- **A crate per platform, under `crates/platform/`.** `platform-core` is the core itself and
  nothing else; what a platform makes of it lives in a crate of its own — `m5/core` for the
  panel and the carried crossings, `web/core` and `web/bridge` for the browser, with each
  platform's untestable shell (`m5/m5plus`) beside its testable core. The alternative was a
  second core crate named apart from the first, which would have prefixed a name to
  disambiguate it rather than to say what it holds.
- **The projection is a trait, not a second core.** A shell implements `Shell`: where its
  crossings come from, and how to project what it shows from the model. `Lookout<S>` is then
  one `App` over one `Event` and one `Model`, with `view` delegating to `S`. The state cannot
  fork, because there is only one of it.
- **One fix shape, in a cross-platform `model`.** A browser's fix already has a type:
  `shared::sensor::Gps`, which `/record` sends and the store keeps. A core taking it reads a
  live browser and a replayed session without translating between two spellings of one thing.
  It moves to a `model` crate holding what a device and the store both use, and `shared` keeps
  the versioned envelope around it. Today's `model` is the store's schema: it depends on
  `medallion`, and so on arrow, which neither Xtensa nor wasm can build. So it becomes
  `medallion-model` and frees the name. `predictor::Sample` was the alternative and needs no
  new type at all, but it names its coordinates `x` and `y`, and a shell that swaps them
  reports a position off Somalia that nothing can catch.
- **Positions in the web ViewModel.** A `Prediction` carries a `CrossingId`, a distance, and
  an arrival instant, but no position. The core already holds the point set, so it resolves
  ids to lat/lon as it projects the web view, leaving `Prediction` as it is.
- **The crossings are fetched, not baked.** The device carries its set in flash because flash
  is what it has; the browser fetches one as a static asset, and is then free of the size flash
  allows, at the cost of a load path.
- **The browser fetches `[[id, lat, lon], …]` and holds a `Vec`.** Each platform reads the set
  the way that suits it: the device scans packed columns where they lie, since copying them
  would cost RAM it has better uses for, and the browser parses JSON into
  `Vec<Crossing<Float>>`, which costs the same 12 bytes a point either way in a process that
  has just allocated the response. About 180 KB, and a third of that compressed.
- **`predictor::Crossings` is the seam.** The core names the trait, not a representation, so
  changing what the browser fetches touches one implementation and nothing else. That is what
  makes JSON affordable as a first answer: the packed format, GeoArrow and anything else stay
  open. GeoArrow is the one to weigh again, because the packed format is already its separated
  point encoding with an id column and no envelope — but reading it needs arrow, which will not
  build for Xtensa at all and, in wasm, costs more than the 67 KB it would be reading. Tracks
  are the change that reopens this: a linestring is where hand-parsing stops being twenty lines.
- **wasm-pack, into the server's static dir.** Trunk owns a page and `server` serves three, so
  the widget is a library those pages load. A `just` recipe builds it for local work and the
  Docker builder stage builds it for a deploy, which keeps generated files out of version
  control.
- **A custom element is the shell.** It owns the wasm instance, the clock, and the canvas in a
  shadow root, so a page adds the predictor by writing one tag. The core stays pure, and the
  element is left with nothing but I/O — see
  [2026-09-19-web-component-shell.md](2026-09-19-web-component-shell.md), which sketches this
  against the device panel rather than the web view.
- **The element owns the core; the page owns the source.** Positions arrive through a method
  the element exposes, so `/live` and `/kiosk` differ in where they read them and in nothing
  else, and a replay is as easy to drive as a fix.
- **The bridge is crux's, over JSON.** `process_event` and `view` go out through
  `wasm-bindgen` as strings, and the shell reads them with `JSON.parse`. Bincode was the first
  choice, but crux 0.16.2 generates TypeScript by shelling out to `pnpm install` and
  `pnpm exec tsc` — a Node toolchain locally and in the Docker builder, for a site that is
  hand-written pages. `BridgeWithSerializer` takes any serde serializer, so `serde_json` buys
  the same bridge for none of that. The cost is that nothing checks the shapes across the
  boundary, which `web-bridge`'s tests cover instead: they assert the exact JSON of an event,
  a request and a view. The crate holding the bridge is `web-bridge`, since `shared` already
  holds the telemetry wire models, and the core behind it is `web-core`.
- **Repaint on `Render`, not on dispatch.** The core answers an event that moved nothing with
  no request at all, which is what stops a replay at speed from redrawing the canvas per
  sample. The shell honours the empty answer.
- **One wasm instance per page.** `init()` is hoisted to module scope so several elements share
  a load, and the first paint waits on that promise, since the browser cannot await
  `connectedCallback`.
- **The kiosk replays an exported session.** The fly deploy has no store, so a recipe exports a
  recorded session to a file served with the site, and the page replays it in the browser.
- **`crux_core` stays pinned at `=0.16.2`.** 0.20 reboots the board (see `docs/device.md`), so
  the shared core stays on the pinned version, and the web shell with it.
- **`/live` shows, `/record` records.** Moving the recording page to `/record` is a move, not a
  rewrite, and `/live` predicts from browser GPS while queueing nothing.
- **A page is a directory.** `server` serves the static dir as it finds it, so `/live` is
  `static/live/index.html` and the bare path redirects to `/live/`. That keeps every page a
  file on disk rather than a route in the binary.

### Tasks

Three phases, so an integration failure shows early. The first carries a throwaway counter
core the whole way — through the bridge and the element to the deployed site — because every
part of that path is new here. The second puts the predictor behind it. The third adds the
remaining pages.

#### Phase 1 — the whole path, over a counter core

- [x] Add a counter core: a `Tick` in, a count out as text. Phase 2 throws it away and keeps
      the bridge and the element it proved.
- [x] Add a bridge crate exposing that core to `wasm-bindgen`, with typegen for its event and
      ViewModel. Over JSON, and so without typegen — see the choice above.
- [x] Build the wasm with a `just` recipe, into the server's static dir.
- [x] Define the custom element: it loads the wasm, runs the tick, and repaints on `Render`.
- [x] Serve it at `/live`, showing the count.
- [x] Build the wasm in the Docker builder stage, and deploy.

#### Phase 2 — the predictor behind it

- [x] Split `platform-core` into a shared prediction core and a device-panel projection,
      leaving the device shell as it is. The shell's own code is unchanged bar its imports and
      one type parameter, but it moved to `crates/platform/m5/m5plus` and gained an `m5-core`
      dependency. Nothing here can build it: it needs `just m5plus-build-release`.
- [x] Split `model` in two: `medallion-model` for the store's schema, and a `model` holding
      what a device and the store share. Only the fix has moved, which is what this phase
      needs; what else belongs on the far side of that line is phase 4's.
- [x] Take a position as an event, alongside an NMEA sentence.
- [x] Project a web ViewModel: current position, speed, and each prediction's lat/lon,
      distance, and arrival.
- [ ] Have the core ask for its crossings instead of being handed them. `Shell::crossings()`
      is called when the model is built, which suits flash and not a download. Replace it with
      a second effect beside `Render`: the core requests a set, the shell answers with bytes —
      immediately on the device, where they are in flash, and after a fetch in a browser. The
      model then starts with no predictor, so decide what the core does in the meantime: a fix
      arriving first should still move `here` and predict nothing, which is either an `Option`
      or a small state machine, and the work will say which. This is also the first effect with
      a response, so `handle_response` stops being unused and the bridge has to expose it.
      Watch the size: a response through the JSON bridge is a number per byte, and the set is
      69 KB. The device shell has to answer the new effect too, and nothing here can build it.
- [-] Let the point set reader borrow owned bytes, so the core can scan a fetched set. Moot:
      the browser holds a `Vec<Crossing<Float>>` rather than reading packed bytes, so nothing
      fetched is borrowed.
- [ ] Serve the crossings as a JSON static asset, and answer the core's request with them.
      Add a `CompressionLayer` while here: 180 KB of coordinates is the first response big
      enough to notice, and it covers every other response too.
- [ ] Swap the counter core for the predictor, and feed `/live` from browser geolocation.
- [ ] Draw the canvas with D3: the centre dot sized by speed, the predictions placed by a
      hyperbolic mapping, and the radius standing for the maximum distance.

#### Phase 3 — the rest of the site

- [ ] Add a recipe exporting a recorded session, and `/kiosk` replaying it.
- [ ] Move the recording page to `/record`, and make `/` a summary linking to the three pages.
- [ ] Fold what holds from `docs/2026-09-19-web-component-shell.md` into the code and its docs,
      and delete the note. Its shape is the slice; its plumbing moves with the typegen build.

#### Phase 4 — store rules out of shared types

Splitting the core by platform found store rules inside types that are not the store's. None of
it is needed for the pages to work, so it comes last. Left undone, the next core written has to
import arrow to name a crossing.

- [ ] Decide what a crossing is, and keep one of it. There are two ids — a `String` in
      `medallion-model`, whose constructor refuses anything that could not name a partition,
      and a `u32` in `predictor`, which is what a device has room for — and `WaterCrossingRow`
      carries both, as `crossing_id` and `crossing_short_id`, so the store already treats them
      as one thing named twice. There are also two `Crossing` types: `model`'s, which is what a
      source reports, and `predictor`'s, which is the same crossing in the float a scan
      measures in. Collapsing those needs `Measure` to move as well, which is the same question
      one level down. The `String` id cannot move to `model` while it validates a medallion
      rule. So: is an id a name for a crossing that also suits a partition, or a partition
      value that also names a crossing? Answer that, then move them.
- [ ] Split `Shell` by what a platform can do, not by what it happens to have. One kind is
      standalone: it brings its crossings, asks for nothing, and needs only somewhere to send
      fixes. That is the board. The other is connected: it can call out, so it can be asked for
      a set it does not hold. That is a browser, and later a board with a radio. One trait
      covers both today, and each implementation answers `None` to the half that is not its
      own — the device cannot be told its crossings, the browser carries none. The device also
      builds an `Effect::Crossings` it can never receive, and its shell carries a match arm for
      an effect that never arrives. The thing to work out is what one core does with two of
      these, since crux builds one effect enum per app and a standalone shell's effects are a
      subset of a connected one's.
- [ ] Find the same pattern elsewhere: a type everything needs, holding a constraint only the
      store has. It hides until something that cannot build arrow — a device, a browser —
      imports one. Such a type belongs in `model`, and the store's rule about it belongs in a
      `medallion-model` type wrapping it. Read every public type in `medallion-model` against
      that line, and move the ones that fall outside it.
