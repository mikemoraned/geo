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

The app embeds the Crux app as a WASM widget that takes injected GPS, and draws a canvas:

* a dot in the middle for where we are, its size proportional to our estimated speed
* the predicted crossings as small dots, placed by a hyperbolic mapping that gives nearer
  distances more of the canvas. Nothing is drawn beyond a maximum distance, so a dot on the
  rim is a crossing at that maximum.

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
- **The kiosk replays an exported session.** The fly deploy has no store, so a recipe exports a
  recorded session to a file served with the site, and the page replays it in the browser.
- **`crux_core` stays pinned at `=0.16.2`.** 0.20 reboots the board (see `docs/device.md`), so
  the shared core stays on the pinned version, and the web shell with it.
- **`/live` shows, `/record` records.** Moving the recording page to `/record` is a move, not a
  rewrite, and `/live` predicts from browser GPS while queueing nothing.

### Tasks

- [ ] Split `platform-core` into a shared prediction core and a device-panel projection,
      leaving the device shell as it is.
- [ ] Take a position as an event, alongside an NMEA sentence.
- [ ] Project a web ViewModel: current position, speed, and each prediction's lat/lon,
      distance, and arrival.
- [ ] Let the point set reader borrow owned bytes, so the core can scan a fetched set.
- [ ] Add a web shell crate, built to wasm by a `just` recipe and by the Docker builder stage.
- [ ] Serve the packed crossings as a static asset.
- [ ] Draw the canvas with D3: the centre dot sized by speed, the predictions placed by a
      hyperbolic mapping, and the radius standing for the maximum distance.
- [ ] Move the recording page to `/record`, and make `/` a summary linking to the three pages.
- [ ] Add `/live`, feeding browser geolocation into the widget.
- [ ] Add a recipe exporting a recorded session, and `/kiosk` replaying it.
