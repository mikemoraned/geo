# Current Slice: mike is on a train getting data

### Target

The main thing I want to get is some real sensor data from being on a real train journey:
* periodic gps snapshot
* accelerometer data

### Architecture

I think the minimum thing required is:
1. A browser frontend that periodically samples gps and accelerometer data, and sends them on a websocket to backend service, stamped with a timestamp, and random. The frontend should generate a random uuid which it uses as it's identity, if it doesn't already have one persisted in a cookie. It should send this id on all samples. Samples should be simply JSON.
2. A rust web server:
    * providing the basic web-page for front-end
    * listening on the websocket and saving the telemetry data to a redis queue
3. A rust cli which can listen on this same queue and empty it in to a rerun.io file
4. This should then be visualisable in rerun

Note:
* the website is deployed to fly.io (e.g. https://lookout.fly.dev) and the redis is deployed on upstash.com as a db called `lookout-telemetry`
* the cli which empties the queue into rerun.io is running locally on my laptop

We should re-use as much style of implementation as used in https://github.com/mikemoraned/bobby

### Decisions

* **Topology** (per the Note above): the frontend + web server are deployed to **fly.io**
  (`lookout.fly.dev`); the queue is **upstash** redis (db `lookout-telemetry`); the
  `recorder` cli runs **locally on the laptop** and drains upstash into a `.rrd`.
* **Real source is a phone, not the laptop.** Per the Learned Constraints in target.md the
  M3 laptop has no accelerometer, so the sensor is an iPhone running the frontend on the
  train.
* **HTTPS comes free from fly.io.** `lookout.fly.dev` serves a real, publicly-trusted cert,
  which satisfies the secure-context requirement of Geolocation and `DeviceMotionEvent`.
  No mkcert / cert-install / LAN / hotspot needed — the phone just loads the public URL,
  and the websocket is `wss://lookout.fly.dev/ws`. (This supersedes the deferred LAN/cert
  work noted in target.md.)
* **Capture needs internet on the phone.** The phone reaches fly.io (and thus upstash) over
  the public internet, so it needs **cellular** during the journey — train wifi is unreliable
  and dead zones will drop the connection. **Risk/mitigation:** the frontend should buffer
  unsent samples locally and flush on websocket reconnect, so dead zones lose nothing.
  Whether to build buffering now or accept gaps in a first pass is an open call (see Tasks).
* **Crate layout** (bobby-inspired flat `crates/`): add a `shared` crate for the sample
  model (reused by server + cli), keep the existing `server` crate, and add a `recorder`
  cli crate that drains redis into a `.rrd`.
* **Sample model** (`shared`): `{ id: Uuid, t: <epoch millis>, gps: Option<{lat,lon,alt,acc}>,
  accel: Option<{x,y,z}> }`, serde JSON. gps and accel are independent (they arrive at
  different rates), each optional so a sample can carry either or both.
* **Identity**: frontend generates a `crypto.randomUUID()` on first load, persists it in a
  cookie, and stamps every sample with it. rerun entity paths are namespaced per device:
  `/device/{id}/accel/{x,y,z}` and `/device/{id}/gps`.
* **Queue**: an upstash redis **list** — the server `LPUSH`es JSON samples, the cli `BRPOP`s
  to drain. Simple and matches "empty it into a rerun file". Both server and cli connect to
  the same upstash db over TLS (`rediss://…`), reading the URL from a `LOOKOUT_REDIS_URL`
  env var.
* **Secrets follow bobby's 1Password pattern.** The upstash TCP URL lives as an item in the
  `Dev` 1Password vault (e.g. `lookout-upstash-redis-url`, value in its `password` field).
  Checked-in `deploy/*.env` files hold only `op://` **references**, never values, e.g.
  `LOOKOUT_REDIS_URL=op://Dev/lookout-upstash-redis-url/password`. Local runs (dev server,
  recorder cli) wrap the command in `op run --env-file=deploy/lookout.env -- …`; the fly
  deploy pushes the resolved value with `fly secrets set LOOKOUT_REDIS_URL="$(op read
  'op://Dev/lookout-upstash-redis-url/password')"`. No secret values are committed.
* **Deploy setup**: follow the existing repo `backend/` fly pattern (Dockerfile + fly.toml)
  for the `server` crate.
* **CLI is a batch drainer**: run during or after the journey (needs internet to reach
  upstash); it blocks on the queue, writes accel as three scalar time series and gps as
  lat/lon scalars (plus rerun geo points if easy), using sample `t` as the timeline, and
  flushes the `.rrd` on Ctrl-C / empty.
* **Sampling rates**: accelerometer from `devicemotion` events as they fire; gps via
  `navigator.geolocation.watchPosition` (or a periodic `getCurrentPosition`). No SolidJS —
  vanilla HTML/JS, consistent with the previous spike.

### Tasks

Keep it compiling at every step and re-verify as you go. Ordered so each phase is a
verifiable milestone.

**Phase 1 — webapp deployed to fly.io, gathering data (not yet over websocket):**

* [ ] Frontend: persist a `crypto.randomUUID()` device id in a cookie; sample `devicemotion`
      (accel) and `geolocation.watchPosition` (gps); show live status (id, counts, last
      gps/accel). Gather and display only — do **not** open/send the websocket yet.
* [ ] Add a Dockerfile + fly.toml for the `server` crate (following the repo `backend/`
      pattern) serving the static page; deploy to `lookout.fly.dev`.
* [ ] Confirm on the phone: load `https://lookout.fly.dev`, grant motion + geolocation
      permissions, and see the page actively gathering data.

**Phase 2 — websocket → redis (verify samples land in redis):**

* [ ] Add a `shared` crate with the `Sample` model (id / t / optional gps / optional accel)
      and serde JSON round-trip tests.
* [ ] Set up secrets the bobby way: create the `lookout-upstash-redis-url` item in the `Dev`
      1Password vault and a checked-in `deploy/lookout.env` holding
      `LOOKOUT_REDIS_URL=op://Dev/lookout-upstash-redis-url/password` (reference only).
* [ ] Add `redis` to the `server` crate; connect to upstash via `LOOKOUT_REDIS_URL`. On each
      received websocket sample, deserialize into `Sample` and `LPUSH` the JSON onto the
      `lookout-telemetry` list. Log queue depth. Run locally via
      `op run --env-file=deploy/lookout.env -- cargo run -p server` to verify against upstash.
* [ ] Frontend: open the websocket (`wss://lookout.fly.dev/ws`) and send timestamped JSON
      `Sample`s. Handle reconnect on the flaky-connection path; decide whether to buffer
      unsent samples locally and flush on reconnect now, or accept dead-zone gaps in a first
      pass (see Decisions).
* [ ] Redeploy to fly with the secret pushed via
      `fly secrets set LOOKOUT_REDIS_URL="$(op read 'op://Dev/lookout-upstash-redis-url/password')"`.
      Confirm from the phone that samples appear in the upstash `lookout-telemetry` queue
      (manual verification in the upstash console).

**Phase 3 — local cli drains redis → rerun:**

* [ ] Add a `recorder` cli crate that connects to the upstash list via `LOOKOUT_REDIS_URL`
      (run via `op run --env-file=deploy/lookout.env -- …`), `BRPOP`s and deserializes
      `Sample`s, and logs them to a `.rrd` via the rerun SDK (accel x/y/z scalars, gps
      lat/lon, per-device entity paths, `t` as timeline). Flush on Ctrl-C / empty.
* [ ] End-to-end: take a real train trip with the phone capturing gps + accel to upstash,
      then run the local `recorder` to drain into a `.rrd` and open it in the rerun viewer,
      confirming the accel curves and gps track are visible.

**End-to-end on a real journey:**

* [ ] Take a real train trip with the phone capturing gps + accel to upstash; then run the
      local `recorder` to drain upstash into a `.rrd`.
* [ ] Open the resulting `.rrd` in the rerun viewer and confirm the accel curves and gps
      track are visible.
