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
- **`d3-geo` for the direction, `d3-scale` for how far out.** A crossing is drawn where it
  lies, which needs a bearing from where we are, and a bearing is geodesy rather than
  arithmetic — `geoAzimuthalEquidistant` centred on the fix gives one by construction. How far
  out is `scaleSymlog`, over the core's own distance: d3 has no scale called hyperbolic, and
  symmetric-log is the one that does what that asks for — near distances spread, far ones
  compressed, and unlike `scaleLog` it has somewhere to put a crossing directly underneath us.
  Its `constant` is where the curve stops being straight, and is the dial for how much of the
  picture the near field takes. Both are files in `static/vendor` named for the version they
  are, not CDN imports, so the page depends on nothing at runtime it does not serve itself and
  an upgrade shows up as a filename.
- **wasm-pack, into the server's static dir.** Trunk owns a page and `server` serves three, so
  the widget is a library those pages load. A `just` recipe builds it for local work and the
  Docker builder stage builds it for a deploy, which keeps generated files out of version
  control.
- **A custom element is the shell.** It owns the wasm instance and the canvas in a shadow
  root, so a page adds the predictor by writing one tag. The core stays pure, and the element
  is left with nothing but I/O — see [web.md](web.md).
- **The bridge is crux's, over JSON.** `process_event` and `view` go out through
  `wasm-bindgen` as strings, and the shell reads them with `JSON.parse`. Bincode was the first
  choice, but crux 0.16.2 generates TypeScript by shelling out to `pnpm install` and
  `pnpm exec tsc` — a Node toolchain locally and in the Docker builder, for a site that is
  hand-written pages. `BridgeWithSerializer` takes any serde serializer, so `serde_json` buys
  the same bridge for none of that. The cost is that nothing checks the shapes across the
  boundary, which `web-bridge`'s tests cover instead: they assert the exact JSON of an event,
  a request and a view. The crate holding the bridge is `web-bridge`, since `shared` already
  holds the telemetry wire models, and the core behind it is `web-core`.
- **Two sources of time, and the clock takes the later.** A fix carries one and a tick carries
  one, and they are not the same clock: a `watchPosition` fix is stamped when it was taken,
  seconds before the browser delivers it. A clock that refused anything behind it would run on
  with the ticks and throw away every fix that followed. So a fix is ordered against the fix it
  replaces — what makes one stale is a newer one — and the clock advances to the later of
  whatever arrives. The device is unaffected: its fixes and its time come from the receiver.
- **Repaint on `Render`, not on dispatch.** The core answers an event that moved nothing with
  no request at all, which is what stops a replay at speed from redrawing the canvas per
  sample. The shell honours the empty answer.
- **One wasm instance per page.** `init()` is hoisted to module scope so several elements share
  a load, and the first paint waits on that promise, since the browser cannot await
  `connectedCallback`.
- **The kiosk replays exported sessions, and which ones is decided in gold.** The fly deploy
  has no store, so what it replays has to be packed into a file served with the site — and if
  a run has to choose anyway, it should choose the sessions worth watching rather than the
  first one to hand. A session is worth watching if it passes crossings, and silver already
  says which: `session_crossing` holds a row per session per crossing passed, so the choosing
  is a count over a dataset that exists rather than a second matching pass.
- **A session replays in a minute, whatever it took to record.** A train takes minutes between
  crossings and nobody watches a kiosk for minutes, so the recorded intervals are ignored and
  the page spends a minute on each session: the delay between samples is that minute divided by
  how many the session has, worked out per session as it starts. Sessions differ by a factor of
  four in length, so one delay for all of them would leave the long ones interminable and the
  short ones over before they read. The samples keep their recorded timestamps, so the speed
  the core derives and the arrivals it predicts come from the real gaps between fixes; only the
  watching is sped up.
- **The replay is driven by the clock, not by a timer.** Each frame works out how far into the
  minute it is and sends the sample that far through the session, so the page runs as fast as
  the browser will paint and skips fixes where it cannot keep up, rather than queueing them and
  falling steadily further behind. Skipping costs nothing the view can show: a fix not sent is
  a position not drawn, and the one that is sent carries its own instant, so the speed between
  them is still measured over the real interval.
- **The element owns the core; the page owns the source and the clock.** Positions arrive
  through a method the element exposes, so a replay is as easy to drive as a fix. The clock
  arrives the same way, and has to: the core takes
  the later of a fix and a tick, so a page ticking wall time while replaying fixes from last
  month would measure every countdown against today and read them all as long past. `/kiosk`
  therefore sends no ticks: a replayed fix carries the recording's own instant, and at a sample
  every few hundred milliseconds there is nothing a tick between them would add. `/live` ticks,
  because a browser fix is seconds apart and a countdown should shorten in between. So the
  element holds the core, the canvas and the crossings it asks for, and takes positions and
  time through one method; where those come from is each page's business.
- **One event starts the core, and starting is all it is.** `Reset` puts the model back to
  what it was built as, which is also what decides whether there are crossings to ask for — so
  a shell sends it when it comes up, and again whenever what follows has nothing to do with
  what came before. That second case is the kiosk: the journeys are chosen by how many
  crossings they pass, not by when they happened, so one recorded in the morning can follow one
  recorded that evening, and looping back to the first always goes backwards. A fix behind the
  last one is refused, and rightly — so each journey begins by saying the last one is over, and
  the page then sends every sample exactly as it was recorded. The alternative was for the page
  to shift each journey onto the end of the one before, which works and puts arithmetic about
  clocks in a shell that should not be doing any.
- **The replay never ends.** It runs each chosen session in turn and then starts again, since a
  kiosk is left running and a screen that stops is one that looks broken.
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
- [x] Have the core ask for its crossings instead of being handed them. `Shell::crossings()`
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
- [x] Serve the crossings as a JSON static asset, and answer the core's request with them.
      Add a `CompressionLayer` while here: 180 KB of coordinates is the first response big
      enough to notice, and it covers every other response too.
- [x] Swap the counter core for the predictor, and feed `/live` from browser geolocation.
- [x] Draw the canvas with D3: the centre dot sized by speed, the predictions placed by a
      hyperbolic mapping, and the radius standing for the maximum distance. `d3-geo` is used
      for the projection and `d3-scale` for the radial mapping; the distances themselves are
      the core's. Drawn against a standing start and a single fix, never against a journey, so
      what it looks like in motion is untested.

#### Phase 3 — the rest of the site

- [x] Add a gold step choosing the sessions worth replaying, and writing them as
      `sessions.json`. It counts the crossings each recorded session passed, which is a group
      over silver `session_crossing`; keeps those passing at least `--min-crossings`; sorts by
      that count and keeps the first `--max-sessions`; and writes each with the samples that
      replay it. Both are arguments, defaulted in the recipe that runs it at 5 and 3. Versioned
      and adopted as `pack_crossings` does, into `sessions.version`, so the page and any build
      read the same recording.
- [x] Serve `sessions.json` and add `/kiosk` replaying it: each session in a minute, in turn,
      round again from the first. The shortest of the three has 252 samples and the longest
      967, so that is a sample every 240ms against one every 62ms — and each one scans the
      whole crossing set and redraws, so watch that the fastest still keeps up. First sight of the canvas against real movement, so
      correct there what phase 2 could only guess at: how often the picture should redraw, how
      much of it the near field should take — the scale's `constant` — and whether a dot
      reaching the rim reads as something approaching.
- [x] Move the recording page to `/record`, and make `/` a summary linking to the three pages.
- [x] Fold what holds from `docs/2026-09-19-web-component-shell.md` into the code and its docs,
      and delete the note. What held is in `docs/web.md` and in the element's own comments;
      what did not is most of the rest — it sketched three functions over Bincode with
      generated bindings, one effect, and a panel of strings, against a core that now has two
      functions over JSON, two effects, and a view of positions.

#### Phase 4 — refactors

##### Split model layers

Splitting the core by platform found store rules inside types that are not the store's. None of
it is needed for the pages to work, so it comes last. Left undone, the next core written has to
import arrow to name a crossing.

- [x] Decide what a crossing is, and keep one of it. There are two ids — a `String` in
      `medallion-model`, whose constructor refuses anything that could not name a partition,
      and a `u32` in `predictor`, which is what a device has room for — and `WaterCrossingRow`
      carries both, as `crossing_id` and `crossing_short_id`, so the store already treats them
      as one thing named twice. There are also two `Crossing` types: `model`'s, which is what a
      source reports, and `predictor`'s, which is the same crossing in the float a scan
      measures in. Collapsing those needs `Measure` to move as well, which is the same question
      one level down. The `String` id cannot move to `model` while it validates a medallion
      rule. So: is an id a name for a crossing that also suits a partition, or a partition
      value that also names a crossing? Answer that, then move them.

      **A name for a crossing that also suits a partition.** Two names, one of each, both in
      `model`: `CrossingId` says what the crossing is made of, and `CrossingCompactId` is the
      same crossing in the four bytes a device has room for. A crossing has two names because
      the places holding one have different room for it, and neither name is the store's.
      The rule the store seemed to own turned out to be the name's: an id is written out as a
      directory, asked for in a URL and read by a person, so `CrossingId` refuses what any of
      those would misread, and `medallion-model` holds no wrapper around it — a wrapper would
      be the second representation this task exists to remove. Collapsed onto those two names:
      `crossings::PackedId`, `predictor::CrossingId`, and the bare `u32` on `WaterCrossingRow`.
      The store's column is renamed to `crossing_compact_id` by the task below.

      Two crossings, for the same reason. `Crossing` is the name and the place, which is what
      anything with room for it holds; `CrossingCompact<T>` is the crossing where there is not
      room — named in four bytes, measured in whatever float the holder scans in, and sent as
      three numbers rather than three named fields. Those are one decision, so they are one
      type, and the positional serialisation is `CrossingCompactRow` against it rather than a
      surprise default on the general crossing, which is not serialised at all. `Measure`,
      `position` and `CoordinateError` moved to `model` with them. What a derivation needs
      beyond a crossing it adds beside one: `crossings::silver` adds the compact id and the
      extraction, and `session_crossings::matching` adds the same place in projected metres —
      which is what its `at` had meant, against a `lat_lon` that was the crossing's own
      position under another name.
- [x] Finish the rename: nothing calls a compact id short. Silver was rewritten and both gold
      artefacts repacked from it. Re-deriving `water_crossing` moved one crossing of 5,760 —
      a new id at a position 7m off — from the same extract, so the collapse is not
      reproducible run to run. Nothing here caused it and nothing here chases it.
- [x] Find the same pattern elsewhere: a type everything needs, holding a constraint only the
      store has. It hides until something that cannot build arrow — a device, a browser —
      imports one. Such a type belongs in `model`, with as much of the constraint as is the
      type's own rather than the store's — which is how the crossing id landed, and the first
      thing to ask of each of these. Read every public type in `medallion-model` against that
      line, and move the ones that fall outside it.

      Read, and the line was wrong. "Something that cannot build arrow needs it" is a symptom,
      and finds a type only once a platform importing it exists — one platform too late. The
      criterion is whether the type says what something *is*, and the bias is to describe a
      new entity in the domain crate by default. Under that line the answer is not one type
      but most of them: a session, a sample, a pass, a device, a leg and an extract are all
      described only as store rows today. The sweep, the evidence and the order to move them
      in were `2026-09-21-model-layers.md`, written as this phase started and deleted at the
      end of it: what held is `crates/domain/README.md`, and what each move settled is on the
      move itself.
- [x] Rename `model` to `domain`, first, so every move below lands under the name it keeps.
- [x] Move `Sample`: a fix and the instant it was taken at. Two of the three shapes the note
      names, since `matching::Sample` turned out not to be one: it holds an instant and a
      position in projected metres, read from the store without lat/lon, so it is a timed
      projected point rather than a reported fix. `predictor::Sample<T>` is the third, and is
      the same fix in the measure with what a receiver adds — the `Crossing`/`CrossingCompact`
      relationship again, left for the task below.
- [x] Keep one sample. `predictor::Sample<T>` is gone and `domain::Sample<T>` is the only one:
      `Gps` became generic over the measure and took the two fields a receiver reports and a
      browser has no equivalent for, which is what the two types differed by. The
      `Crossing`/`CrossingCompact` split does not apply here — those need two types because
      their *ids* differ, where a fix differs only by the float it is held in, which a
      generic covers.

      Two things fell out. `accuracy_metres` is now optional, since NMEA reports HDOP and no
      metres: the store's columns are unchanged, and a reported fix without one is malformed,
      so its payload stays in `raw` as an uninterpretable one does. And the conversion between
      precisions is `Sample::to_measure`, which checks the coordinates — a fix read off a wire
      is read unchecked, so that is where an impossible position is stopped rather than being
      scanned against every crossing.

- [x] Rename the `Measure` trait to `Precision`: what it bounds is how finely a coordinate is
      held — `f32` or `f64` — and `Measure` reads as the measurement rather than its
      resolution. Not `Accuracy`, which this codebase already uses for something else and
      uses correctly: `accuracy_metres` is how far out a fix is, as its source judged it,
      which is unrelated to the float it is held in. `Gps::to_measure` and `Sample::to_measure`
      became `to_precision` with it, and the parameter is `P` rather than `T`.

- [x] Move `DeviceId` and `SessionId`. A device mints its own id and a session is derived from
      that id and its start, so both say what something is rather than how the store keeps it.
      The `PartitionValue` rule on `SessionId` is the name's own, as it was for `CrossingId`,
      and is now one rule in `domain::name` rather than a copy per id. The derivation is
      pinned to a session the store already holds, since a change to the namespace or to what
      is hashed would still derive consistently while renaming every session recorded.

- [x] Move `Pass`: a crossing met in a session. `matching` now computes passes and names
      nothing of the store's — it was a prune, a distance and a nearest sample written in
      column names. What the row adds, `silver` adds: the device, which is derivable from the
      session and carried so a partition reads without joining back, and the radius, which is
      how hard the run looked rather than what it found. The 253 passes the store holds
      re-derive identically, column for column.

- [x] Move `Session`, the run of samples itself — and it turned out to be `StartedBy` that
      moved. A session is written three other ways and none is the same thing: `matching`
      holds an id, a device and an envelope; `gold::Replay` an id, a count and its samples;
      `recorder` the samples and the tuning it derived them under. Each selects the columns
      it uses, so a `domain::Session` would have been a type built only to become a row.
      `StartedBy` is the part that was misplaced: what began a session is decided in
      `recorder`, which was importing the store's schema crate to name it. Left as it was:
      the tuning and the envelope, which are how a run was made and a denormalisation of
      what it found.

- [x] Move `DeviceType`, and have `DeviceSessionRow` name it rather than a `String`.
      `DeviceType::as_str` existed, by its own doc, "for storing in a text column", and had
      one caller — so the store's handling of the enum had leaked onto a type a browser
      reports. It is unnecessary: `medallion::fields` traces with
      `enums_without_data_as_strings(true)`, so the row names the set and the column is the
      same text it always was. `DeviceInfo` moved with it, since what a device says about
      itself belongs beside the device. That empties `shared::session`, and `shared` is left
      holding the wire: a versioned message and the sensor payloads it carries.

- [x] Keep one window. The two were `crossings::Bbox`, which parsed a command line and
      checked its corners, and `medallion_model::Bbox`, four public floats a writer filled in
      by hand. One `domain::Bbox` now, and it is georust's `Rect` underneath: a box orders its
      own corners, so the inversion the note warned about cannot arise — that warning was
      hypothetical, since the envelope is built from `bounding_rect` either way. `contains`
      delegates to `Intersects` rather than being written out, which is also the one that
      includes the boundary; `Contains` follows the OGC and excludes it. What the wrapper adds
      is what geo has no way to know: that the numbers are degrees, and the two forms a window
      is written in — a command line's, and the stored column's four named corners, which an
      existing test holds steady.
- [x] Weigh a leg and an extract, which the note lists and does not rank. Neither moves.
      A leg has three shapes — the polled row, the deduped row, and the query's own — and
      they are nearly the same fields, so it is one thing written thrice. But nothing holds
      a leg: `ingest` reads columns and writes columns, and nothing computes over one the
      way `matching` computes over sessions and crossings. It earns a type when something
      reasons about legs, which is matching a session to the train it was on. An extraction
      is not a domain thing at all: it is how reference data got into the store, which is the
      store's business.

      `TrainNumber` did move, being the `DeviceType` case again — a checked type unwrapped to
      a bare integer at the row, in a column that could then hold the zero the type exists to
      refuse. It holds a `u32` rather than a `NonZeroU32` because a schema is traced by
      probing the type with values, zero among them, so a number that cannot be zero cannot
      describe a column.
- [-] Two things in the extract looked like duplicates of what the domain holds. Neither is.
      `ExtractId` names a partition of a store-only dataset, so `medallion::PartitionValue` is
      the rule it is actually held to rather than an indirection through `domain::name` — and
      a test already ties the domain's copy of that rule to medallion's. The manifest's window
      is four scalars by a documented choice, since provenance is read by comparing numbers,
      and it comes from an aggregate over Overture's own boxes: a `Rect` in order already,
      with no user input for a checked window to check.

- [x] Capture the rule from the note in `domain`'s `README.md`, and delete the note. The note
      is the reasoning and the evidence, which goes stale once the moves land; the README is
      the default and the entity-to-projection relationship, which does not.

##### Shell split

- [-] Split `Shell` by what a platform can do, not by what it happens to have. One kind is
      standalone: it brings its crossings, asks for nothing, and needs only somewhere to send
      fixes. That is the board. The other is connected: it can call out, so it can be asked for
      a set it does not hold. That is a browser, and later a board with a radio. One trait
      covers both today, and each implementation answers `None` to the half that is not its
      own — the device cannot be told its crossings, the browser carries none. The device also
      builds an `Effect::Crossings` it can never receive, and its shell carries a match arm for
      an effect that never arrives. The thing to work out is what one core does with two of
      these, since crux builds one effect enum per app and a standalone shell's effects are a
      subset of a connected one's.

      Deferred, with the work in the history rather than the tree: 96ae8094 made the split
      and 026c8ae0 took it back out. What it settled is on those two commits. Two kinds of
      platform are two apps, because crux builds one effect enum per app. The state does not
      fork with them: one `Model` and one `Event` underneath. An effect a platform cannot
      perform costs its shell a dead match arm, where an event it never sends costs nothing.
      That is why the effects split and the events did not. Whoever picks this up starts
      from that diff.

##### The does-it-bring-joy rule

Most comments Claude writes are not worth their space. So none is written by default: what one
would have said belongs in the naming and the structure, and a convention belongs in a README.
Nothing here is the website's, but retro-applying it is a sweep of every crate, so it lands
where the other refactors do.

- [x] Add `.claude/rules/does-it-bring-joy.md`, over `**/*.rs` and `**/*.py`. No comment is
      written by default, in any form — `//`, `///`, `//!` or a docstring. What one would have
      said is carried by naming and structure, with a well-known pattern named as such wherever
      one fits. A convention used or invented is explained in a README or another `.md`, away
      from the code. A comment survives only where specific justification is given for that one
      comment. A doc comment on a public item is no exception, and comes under the same asking.
      Whatever survives goes through `writing-clearly-and-concisely`. Index the rule in
      `CLAUDE.md`, and reconcile `.claude/rules/rust.md`: its doc-comment section reads as how
      to write the ones you are adding, where it should describe the rare one that earned its
      place.

- [ ] Apply it across `apps/lookout/crates/**`, one crate or module per commit. That is 3,482
      doc-comment lines and 248 `//` lines over 154 Rust files, plus the `#` lines in
      `rerun-py` and `medallion-py`. Naming and structure may change to absorb what a comment
      said, which is the point of the rule; each commit keeps its tests green and changes no
      behaviour. A durable fact about the system that no name can hold moves into the app's
      `docs/` rather than going, as `CLAUDE.md` already directs. Out of the sweep:
      `apps/linzer`, the notebooks under `notebooks/` and `questions/`, and `spikes/`. A
      notebook is a record of an exploration where a `#` is often a cell's only narration, and
      `spikes/` is a record by definition.

      Swept so far: `shared` — the wire format and what a reading holds moved to
      `docs/telemetry.md`, the crate's purpose to a README. `summary` — the report's layout
      and what it does not read moved to a README, and the one property it relies on of the
      store to `docs/medallion.md`.

      The sweep taught the rule two sections as it went: text a tool reads — a CLI's help, a
      doctest — is not a comment and needs no case made for it, and the rule stays out of the
      prose, so no `.md` explains it or lists what survived.
