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
- **Positions in the web ViewModel.** A `Prediction` carries a `CrossingId`, a distance, and
  an arrival instant, but no position. The core already holds the point set, so it resolves
  ids to lat/lon as it projects the web view, leaving `Prediction` as it is.
- **The crossings are fetched, not baked.** The device carries its set in flash because flash
  is what it has; the browser fetches a packed set as a static asset. The web is then free of
  the size flash allows, at the cost of a load path: the reader borrows its bytes, so fetched
  bytes need an owned, aligned buffer behind them.
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
- [ ] Build the wasm in the Docker builder stage, and deploy. The Dockerfile is written; the
      deploy is not run. Neither Docker nor a browser runs in the sandbox, so `/live` has been
      proved only through the wasm module itself, under node.

#### Phase 2 — the predictor behind it

- [ ] Split `platform-core` into a shared prediction core and a device-panel projection,
      leaving the device shell as it is.
- [ ] Take a position as an event, alongside an NMEA sentence.
- [ ] Project a web ViewModel: current position, speed, and each prediction's lat/lon,
      distance, and arrival.
- [ ] Let the point set reader borrow owned bytes, so the core can scan a fetched set.
- [ ] Serve the packed crossings as a static asset, and fetch them into the element.
- [ ] Swap the counter core for the predictor, and feed `/live` from browser geolocation.
- [ ] Draw the canvas with D3: the centre dot sized by speed, the predictions placed by a
      hyperbolic mapping, and the radius standing for the maximum distance.

#### Phase 3 — the rest of the site

- [ ] Add a recipe exporting a recorded session, and `/kiosk` replaying it.
- [ ] Move the recording page to `/record`, and make `/` a summary linking to the three pages.
- [ ] Fold what holds from `docs/2026-09-19-web-component-shell.md` into the code and its docs,
      and delete the note. Its shape is the slice; its plumbing moves with the typegen build.
