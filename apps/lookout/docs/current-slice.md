# Current Slice: mike is on a train getting data

### Target

The main thing I want to get to is some real sensor data from being on a real train journey:
* periodic gps snapshot
* accelerometer data

We should bias towards recording as much fidelity of data as we can i.e. record everything the browser gives us. It is acceptable to save a subset of samples if saving all of them would be expensive in storage or introduce latency problems.

### Minimal Architecture

The minimum thing required is something like:
1. A browser frontend that periodically samples gps and accelerometer data, and sends them on a websocket to a backend service, stamped with a timestamp and a device-id. The frontend should generate a random uuid which it uses as it's device-id (if it doesn't already have one persisted in a cookie). Samples should be simple JSON.
2. A rust web server:
    * providing the basic web-page for front-end
    * listening on the websocket and saving the telemetry data to a redis queue
3. A rust cli which can listen on this same queue and empty it in to an archive format
4. This archive format should then be visualisable in rerun

Note:
* the website should be deployed to fly.io (e.g. https://lookout-home.fly.dev) and the redis queue is deployed on upstash.com as a db called `lookout-telemetry`
* the cli which empties the queue into an archive format is running locally on my laptop

The archive format should be sqlite and we use rerun for visualisations. Both kinds of files should be small enough for now to just be checked in to git.

We should have two types of tables in sqlite:
* lossless raw samples: this is a single table which just contains *all* data we got from the queue, stored as json. We'll generally not want to be using this directly but we keep it just in case we want to reprocess later if we missed something.
      * the raw table should have a primary key which is a hash (md5 is enough) of the json, so we can do upserts on same data and not worry about dupes.
* per sensor tables: this is a table per sensor type deduped on (device_id, t) so we can do `INSERT OR IGNORE` style updates. Example schema for acceleration:
```sql
CREATE TABLE accel (
  device_id TEXT NOT NULL,
  t         INTEGER NOT NULL,   -- epoch millis, as sent
  x REAL, y REAL, z REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;
```

Per-sensor tables are a derivation of info in the raw table, not an independent store. We can later rebuild them from raw without touching the queue.

We should re-use as much style of implementation as used in https://github.com/mikemoraned/bobby

### Decisions

* **Topology** (per the Note above): the frontend + web server are deployed to **fly.io**
  (`lookout.fly.dev`); the queue is **upstash** redis (db `lookout-telemetry`); the
  `recorder` cli runs **locally on the laptop** and drains upstash into a `.sqlite` DB.
* **Real source is a phone, not the laptop.** Per the Learned Constraints in target.md the
  M3 laptop has no accelerometer, so the sensor is an iPhone running the frontend on the
  train.
* **HTTPS comes free from fly.io.** `lookout-hom.fly.dev` serves a real, publicly-trusted cert,
  which satisfies the secure-context requirement of Geolocation and `DeviceMotionEvent`.
  No mkcert / cert-install / LAN / hotspot needed — the phone just loads the public URL,
  and the websocket is `wss://lookout-hom.fly.dev/ws`. (This supersedes the deferred LAN/cert
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
  to drain. Both server and cli connect to the same upstash db over TLS (`rediss://…`), 
  reading the URL from a `LOOKOUT_REDIS_URL` env var.
* **Secrets follow bobby's 1Password pattern.** The upstash TCP URL lives as an item in the
  `Dev` 1Password vault (e.g. `lookout-upstash-redis-url`, value in its `password` field).
  Checked-in `deploy/*.env` files hold only `op://` **references**, never values, e.g.
  `LOOKOUT_REDIS_URL=op://Dev/lookout-upstash-redis-url/password`. Local runs (dev server,
  recorder cli) wrap the command in `op run --env-file=deploy/lookout.env -- …`; the fly
  deploy pushes the resolved value with `fly secrets set LOOKOUT_REDIS_URL="$(op read
  'op://Dev/lookout-upstash-redis-url/password')"`. No secret values are committed.
* **Deploy setup**: follow the existing repo `backend/` fly pattern (Dockerfile + fly.toml)
  for the `server` crate.
* **Sampling rates**: accelerometer from `devicemotion` events as they fire; gps via
  `navigator.geolocation.watchPosition` (or a periodic `getCurrentPosition`).

### Tasks

Keep it compiling at every step and re-verify as you go. Ordered so each phase is a
verifiable milestone.

**Phase 1 — webapp deployed to fly.io, gathering data (not yet over websocket):**

* [x] Frontend: persist a `crypto.randomUUID()` device id in a cookie; sample `devicemotion`
      (accel) and `geolocation.watchPosition` (gps); show live status (id, counts, last
      gps/accel). Gather and display only — do **not** open/send the websocket yet.
* [x] Add a Dockerfile + fly.toml for the `server` crate (following the repo `backend/`
      pattern) serving the static page; deploy to `lookout-hom.fly.dev`.
* [x] Confirm on the phone: load `https://lookout-hom.fly.dev`, grant motion + geolocation
      permissions, and see the page actively gathering data.
* [x] Throttle gather/display (and later send) to one sample each per fixed interval
      (`SAMPLE_INTERVAL_MS`, default 500ms) — raw `devicemotion`/`watchPosition` fire far
      too often. (Interval landed at 10000ms; each source also emits a leading sample on
      its first reading rather than waiting a full interval.)

**Phase 2 — websocket → redis (verify samples land in redis):**

* [x] Add a `shared` crate with the `Sample` model (id / t / optional gps / optional accel)
      and serde JSON round-trip tests.
* [x] Set up secrets the bobby way: create the `lookout-upstash-redis-url` item in the `Dev`
      1Password vault and a checked-in `deploy/lookout.env` holding
      `LOOKOUT_REDIS_URL=op://Dev/lookout-upstash-redis-url/password` (reference only).
* [x] Add `redis` to the `server` crate; connect to upstash via `LOOKOUT_REDIS_URL`. On each
      received websocket sample, deserialize into `Sample` and `LPUSH` the JSON onto the
      `lookout-telemetry` list. Log queue depth. Run locally via
      `op run --env-file=deploy/lookout.env -- cargo run -p server` to verify against upstash.
      (Pushing goes through a `SampleSink` port — `RedisSink` for prod — so the `/ws` path is
      covered by a Docker-free integ test with a recording sink. Redis is optional: log-only
      when `LOOKOUT_REDIS_URL` is unset. Verifying against real upstash still needs the
      secret + db, below.)
* [x] Frontend: open the websocket (`wss://lookout.fly.dev/ws`) and send timestamped JSON
      `Sample`s. Handle reconnect on the flaky-connection path; decide whether to buffer
      unsent samples locally and flush on reconnect now, or accept dead-zone gaps in a first
      pass (see Decisions). (Built a best-effort in-memory outbox flushed on (re)connect with
      backoff — not persisted, so a reload or an overflow past MAX_OUTBOX drops oldest.)
* [x] Redeploy to fly with the secret pushed via
      `fly secrets set LOOKOUT_REDIS_URL="$(op read 'op://Dev/lookout-upstash-redis-url/password')"`.
      Confirm from the phone that samples appear in the upstash `lookout-telemetry` queue
      (manual verification in the upstash console). (Root cause of the initial "no data" was a
      bad upstash credential — reset it in upstash + 1Password. Debugging added durable
      machinery: a `/version` endpoint + startup log of the build git hash (via a
      `BUILD_GIT_HASH` build arg), fail-loud exit when `LOOKOUT_REDIS_URL` is set but redis is
      unreachable, and a release-mode `end_to_end` test hitting real upstash (`just
      end_to_end_test`). Also pinned the Docker builder to `rust:slim-bookworm` for a glibc
      mismatch, and added `tls-rustls` redis features for `rediss://`.)

**Phase 3 — local cli drains redis → archive format:**

* [x] Add a `recorder` cli crate that connects to the upstash list via `LOOKOUT_REDIS_URL`
      (run via `op run --env-file=deploy/lookout.env -- …`), `BRPOP`s and deserializes
      `Sample`s, and logs them to a `.rrd` via the rerun SDK (accel x/y/z scalars, gps
      lat/lon, per-device entity paths, `t` as timeline). Flush on Ctrl-C / empty.
      (Queue connect/key/drain extracted to a `telemetry` crate shared by server + recorder.
      Recorder has two subcommands, default **view-latest** — non-destructive `LRANGE` of the
      latest N (default 1000) — and **drain** — destructive `BRPOP` until empty/Ctrl-C — so
      you don't accidentally consume the queue while iterating. gps also logged as rerun geo
      points. Run via `just record` / `just record drain`. Queue read paths covered by a
      `telemetry` `_docker` test; producing a real `.rrd` from live data is the next item.)
* [x] End-to-end: take a real train trip with the phone capturing gps + accel to upstash,
      then run the local `recorder` to drain into a `.rrd` and open it in the rerun viewer,
      confirming the accel curves and gps track are visible.
      * this was sort-of done in that it wasn't drained, just viewed (into ./data/lookout.rrd)
* [ ] change of plan: need to use something else more suitable than rerun format (which isn't an archive format). 
      * We'll use sqlite as an export format. So, we'll change our plan to:
            * [x] `recorder` cli saves to sqlite (keeps the view and drain modes)
                  * we should create two separate tables for each kind of sensor (see "Minimal Architecture")
                  * Went with the full Minimal Architecture: a lossless `raw(md5,json)` table
                    *plus* per-sensor `accel`/`gps` tables (all `INSERT OR IGNORE`). To keep raw
                    lossless, `telemetry` now returns a `RawSample` (verbatim json + `parse()`)
                    rather than a decoded `Sample`. Verified via in-memory store unit tests; still
                    needs a real `just record` run against upstash (sandbox blocks `op`).
            * [ ] new `visualise` cli converts from sqlite into rrd format
                  * it is ok if this is written in python as rerun has better support for things like programatically specifying "blueprints" in python
                  * the visualise code should go in the `visualise` dir which has been initialised as an empty uv project
                  * visualise should select by time and device, not count. So against SQL it should be `--since 7d --devices <uuid list>`

**End-to-end on a real journey:**

* [x] Take a real train trip with the phone capturing gps + accel to upstash
* [ ] Run the local `recorder` to drain upstash into a `.sqlite`. 
* [ ] Convert to `.rrd` via `visualise`.
* [ ] Open the resulting `.rrd` in the rerun viewer and confirm the accel curves and gps track are visible.
