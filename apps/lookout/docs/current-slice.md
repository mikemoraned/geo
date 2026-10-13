# Current Slice: Protect /record to avoid accidental misuse

### Target

Currently, anyone can access <https://lookout-hom.fly.dev/record/>. Right now, very few people should know about this site, but this means technically anyone could be recording their position in my data, and I'd like to stop that being possible.

I'd like to do this by protecting `/record/` route so that only I can access it.

### Straw man

I suggest we do this using Passkeys. The way I'd like it to work is something like:
1. User goes to <https://lookout-hom.fly.dev/record/>
   - **Changed:** without a session, `/record/` redirects to `/signin/`, where the user registers
     or signs in.
2. Server creates a Passkey, with an id, records it in a valkey hashmap as `lookout-passkeys`, and the returns it to the user, with another different short-code shown as something to pass to the Admin, which is unique enough to identify the Passkey.
   - **Changed:** the browser creates the passkey, not the server. The server issues a challenge
     and stores the public key the browser returns. The store is the existing Upstash redis, not a
     separate valkey.
3. Admin runs a small cli which looks-up the Passkey by short-code and adds it to a new `lookout-passkeys-allowed` set which says who's allowed access
4. User visits <https://lookout-hom.fly.dev/record/> again, presents Passkey, which is found in `lookout-passkeys` and also in `lookout-passkeys-allowed`, so they are allowed through
   - **Changed:** a sign-in leaves a session cookie. Later visits present the cookie, not the
     passkey, until the session expires. The cookie also opens `/ws`, which the page writes to.

### The records in redis

A passkey record is a credential that has been registered. It sits in `lookout-passkeys`, a hash
keyed by credential ID, and holds the webauthn-rs `Passkey`, its short code, and when it was
registered. A record grants nothing on its own. A browser signs in only with a passkey whose
credential ID is also in `lookout-passkeys-allowed`, a set.

A session is not a passkey record. A successful sign-in leaves it behind: a cookie in the browser
naming a `lookout-session:<id>` key, which expires with the session.

### Decisions

Decisions taken before starting, as each changes what gets built. The user confirmed these on
2026-10-12:

- **The gate covers `/record/` and `/ws`.** The page holds no data and only writes to `/ws`. A
  gate on the page alone leaves `/ws` open to a script that sends JSON straight to it. A browser
  sends same-origin cookies on the websocket upgrade, so one session cookie gates both. The other
  pages write nothing and stay open.
- **webauthn-rs on the existing axum server, not cot.** webauthn-rs 0.5.5 runs the registration
  and sign-in ceremonies. cot 0.7 is built on axum, but its auth uses passwords and a database and
  has no passkeys. Adopting it rewrites the server and still needs webauthn-rs.
- **Sessions use tower-sessions, with a `SessionStore` impl over the existing `redis` client.** The
  session carries the ceremony state between start and finish, then marks the browser signed in.
  tower-sessions handles the cookie and the expiry, and the impl keeps the server on one redis
  client. Two options lost. `tower-sessions-redis-store` builds on `fred`, which adds a second
  client. A hand-rolled token would reimplement the cookie and expiry handling that tower-sessions
  already has.
- **With no redis configured, the gate is off and the server warns at startup.** Without redis
  the server queues nothing, so an open `/record/` records nothing. `just serve` stays one
  command, as the server README's "redis is optional" requires. In-memory stores would gate local
  development, but the admin CLI cannot reach them to allow a key.

Proposed during decomposition, not yet confirmed:

- **The browser creates the passkey, and the server stores its public key.** In WebAuthn the
  authenticator creates the key pair, and the private key never leaves it. The server issues the
  challenge and verifies what comes back.
- **The short code is 8 random Crockford base32 characters, minted at registration.** A
  credential ID is too long to read out. Eight characters give 2^40 codes, so a guessed code matches
  no record. The admin CLI finds a record by scanning the hash, which holds a handful of entries.
- **Sign-in offers every allowed credential.** `start_passkey_authentication` takes the allowed
  passkeys, so a browser holding only an unallowed passkey fails before it signs anything. The
  discoverable flow needs no list but adds an identify step, and the allowed set holds one or two
  entries.
- **The ceremony state is serialised into the session.** webauthn-rs serialises
  `PasskeyRegistration` and `PasskeyAuthentication` only under `danger-allow-state-serialisation`.
  The feature exists for state held on the server, which a session in redis is.
- **Each sign-in writes the passkey back.** `finish_passkey_authentication` returns a counter and
  backup state, and `Passkey::update_credential` applies them. Synced passkeys, such as
  1Password's, usually report a counter of 0, and webauthn-rs skips the counter check at 0. The
  write-back records the backup state, not a counter for clone detection.
- **A session lasts 30 days from its last request.** A recording runs for hours, and a phone signs
  in once. `Expiry::OnInactivity` renews the session on each request.
- **The relying party comes from `LOOKOUT_ORIGIN`**, which defaults to `http://localhost:3000`.
  WebAuthn binds a credential to its origin, and browsers accept `localhost` over plain HTTP.
  `fly.toml` sets `https://lookout-hom.fly.dev`.
- **Sign-in is a page of its own, `/signin/`.** In this server a page is a directory, so the page
  is static and calls JSON endpoints under `/auth/`. A request for `/record/` without a session
  redirects there. A `/ws` upgrade without one gets 401.
- **`redis` moves to 1.7 and `tower-http` to 0.7 first, each in its own commit.** The lookout
  upgrades rule requires it, since the slice adds code on both. `telemetry`, `recorder`, and
  `motis` share `redis`, so its move runs their Docker tests too.

#### Rejected / deferred

- **cot.** It comes back if the server grows enough pages and forms to pay for a rewrite onto a
  framework.
- **A gate on `/live` and `/kiosk`.** Neither writes. The gate extends to `/kiosk` if the journeys
  it replays become private.

#### Open questions

- Whether iOS Safari has `PublicKeyCredential.parseCreationOptionsFromJSON` and `toJSON()`. If it
  has, `/signin/` passes the webauthn-rs JSON straight through. If not, the page converts
  base64url itself. The throwaway group answers it.
- How an iPhone reaches a development server over HTTPS. WebAuthn needs a secure context, and a
  phone cannot use `localhost`. A tunnel or a deploy to fly are the candidates.
- Whether registration needs a bound. Anyone can register, so a script can fill
  `lookout-passkeys`. A cap on unallowed records, or an expiry on them, would bound it.

### Tasks

#### Refactors / extensions

- [x] Move `redis` to 1.7 across the workspace, and rewrite its TLS feature comment in `Cargo.toml`.
- [x] Run the Docker tests of `telemetry`, `server`, `recorder`, and `motis` against `redis` 1.7.
- [ ] Move `tower-http` to 0.7, in a commit of its own.

#### A throwaway sign-in

The ceremony on an iPhone is the largest unknown, so it runs end to end before the stores exist.
Its memory stores go when the gate lands.

- [ ] Add `webauthn-rs` 0.5 with `danger-allow-state-serialisation`, and `tower-sessions`, to
      `crates/server`.
- [ ] Add `/auth/register/start`, `/auth/register/finish`, `/auth/login/start`, and
      `/auth/login/finish`, over a `MemoryStore` and an in-memory passkey list.
- [ ] Add `static/signin/`, registering and signing in through `navigator.credentials`.
- [ ] Register and sign in from desktop Safari and Chrome against `just serve`.
- [ ] Register and sign in from an iPhone over HTTPS. This waits on the open question of how.

#### The stores

- [ ] Add `PasskeyStore` in `crates/server/src/passkeys.rs`: register, find by short code, allow,
      revoke, list allowed, and update.
- [ ] Test `PasskeyStore` against a containerised redis, as `redis_sink_docker.rs` tests the sink.
- [ ] Add `RedisSessionStore`, a `tower_sessions::SessionStore` over the `redis` client, with the
      session's expiry as its key's TTL.
- [ ] Test `RedisSessionStore` against a containerised redis.

#### The gate

- [ ] Replace the throwaway's memory stores with `PasskeyStore` and `RedisSessionStore`.
- [ ] Leave the gate off when `LOOKOUT_REDIS_URL` is unset, and warn at startup.
- [ ] Mark the session signed in when login finishes, and write the updated passkey back.
- [ ] Serve `/record/` through a layer that redirects to `/signin/?next=/record/` without a session.
- [ ] Return 401 from the `/ws` upgrade without a session, tested in `ws_roundtrip.rs`.
- [ ] Add `/auth/status`, returning whether the session is signed in.
- [ ] Read `LOOKOUT_ORIGIN` in `bin/server.rs`, and set it in `fly.toml`.

#### The admin CLI

- [ ] Add `crates/server/src/bin/passkeys.rs`, with clap subcommands `list`, `allow <code>`, and
      `revoke <code>`.
- [ ] Add `just passkeys *args`, with the redis URL from 1Password.

#### The pages

- [ ] Show the short code on `/signin/` after registration, and tell the user to pass it to the
      admin.
- [ ] On each websocket close in `static/record/app.js`, check `/auth/status`. If signed out, show
      it with a link to `/signin/`.

#### Wrap-up

- [ ] Describe the gate, the session store, and the CLI in `crates/server/README.md`.
- [ ] Describe signing in, and the signed-out state of `/record/`, in `docs/web.md`.
- [ ] Run `just test-no-docker` and the server's Docker tests.
- [ ] Deploy, register from the phone, allow it with `just passkeys allow`, and record a journey.
- [ ] Set 1Password as the iPhone's passkey provider, under AutoFill in Settings.
- [ ] Register through 1Password on the laptop, and allow it. Sign in from the iPhone without
      registering again.
- [ ] From a second browser, check that `/record/` redirects and that `/ws` refuses the upgrade.
