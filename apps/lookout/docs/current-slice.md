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
   - **Changed:** the passkey waits under its short code as a pending passkey. The admin has an
     hour to allow it before it expires.
3. Admin runs a small cli which looks-up the Passkey by short-code and adds it to a new `lookout-passkeys-allowed` set which says who's allowed access
   - **Changed:** allowing moves the pending passkey into `lookout-passkeys`, which holds only
     allowed passkeys, so no separate set exists.
4. User visits <https://lookout-hom.fly.dev/record/> again, presents Passkey, which is found in `lookout-passkeys` and also in `lookout-passkeys-allowed`, so they are allowed through
   - **Changed:** a sign-in leaves a session cookie. Later visits present the cookie, not the
     passkey, until the session expires. The cookie also opens `/ws`, which the page writes to.
   - **Changed:** as a sign-in starts, the server names no passkey. The browser offers the one it
     holds for the site, and the server looks it up by its credential ID.

### The records

The site has one account, `recording`, named for its purpose. A device gets access to it in
three steps. It registers a passkey, which waits as a pending passkey under a short code. The
admin allows the code. The device then signs in, which leaves a session. Every request to
`/record/` or `/ws` checks the session and the passkey that opened it.

A **pending passkey** has been registered but not allowed, and grants nothing. It sits under
`lookout-passkey-pending:<code>`, keyed by its short code, and expires an hour after
registration.

An **allowed passkey** can sign in. Allowing a pending passkey moves it into `lookout-passkeys`, a
hash keyed by credential ID. Each entry holds the webauthn-rs `Passkey`, its short code, an
optional label, and when it was registered, allowed, and last used. It stays until the admin
revokes it.

A **ceremony** is the server's half of a registration or a sign-in: the challenge it issued, and
what it expects back. It sits under `lookout-ceremony:<id>`, a random ID, for 10 minutes. A finish
takes it out, so each ceremony finishes at most once.

A **session** is what a successful sign-in leaves: a cookie in the browser naming a
`lookout-session:<id>` key. The key holds the credential ID of the passkey that signed in, and
expires 30 days after the session's last use.

Without redis, the server keeps the same records in memory.

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
- **Sessions use tower-sessions, with a `SessionStore` impl over the existing `redis` client.**
  tower-sessions handles the cookie and the expiry, and the impl keeps the server on one redis
  client. Two options lost. `tower-sessions-redis-store` builds on `fred`, which adds a second
  client. A hand-rolled token would reimplement the cookie and expiry handling that tower-sessions
  already has. Since 2026-10-13 only a sign-in creates a session, and a ceremony waits under its
  own ID instead of in the session.

The user confirmed these on 2026-10-13, after a security review of the first plan:

- **webauthn-rs builds its own OpenSSL, and `passkey-auth` is the fallback.** webauthn-rs 0.5
  depends on OpenSSL, with no feature to drop it. The `vendored` feature compiles OpenSSL into the
  binary, so neither the host nor the runtime image needs a system OpenSSL. The Docker builder
  installs `make` and `perl` for that build. If the build fails, locally or under Docker,
  `passkey-auth` 0.1 replaces webauthn-rs. It is pure Rust and makes every check this slice needs.
  One author wrote it in two days, and no other project appears to use it. That risk is
  acceptable at this project's scope.
- **Anyone can register, and only the admin allows.** A registration stores a pending passkey
  under a fresh short code, which `/signin/` shows. The admin allows it with
  `just passkeys allow <code>`. A pending passkey expires an hour after registration, so a script
  registering in a loop holds at most an hour of records. Invites lost: the admin would mint a
  code first, and a registration would need that code. Invites store nothing for strangers but
  reverse the straw man's order.
- **The gate is always on. Without redis, the stores live in memory.** If production lost its
  redis secret, a gate that turned off without redis would fail open. Memory stores keep
  `just serve` one command, and a restart forgets them.
- **Without redis, a passkey registered for `localhost` is allowed at once.** The admin CLI
  reaches redis, not a server's memory, so nothing else can allow a passkey locally. Production's
  origin is not `localhost`, so production without redis allows nothing. A local redis lost: it
  keeps one way to allow, but needs Docker to sign in locally.
- **A session lasts 30 days from its last use.** Each gated request renews it. tower-sessions
  renews an `OnInactivity` session only under
  [`with_always_save(true)`](https://docs.rs/tower-sessions/0.15.0/tower_sessions/service/struct.SessionManagerLayer.html#method.with_always_save),
  at one redis write per request. A fixed 30 days from sign-in lost: a phone in regular use would
  still sign in again each month.
- **No credential ID leaves the server before sign-in.** A list of credential IDs sent to a
  caller who has not signed in
  [leaks them](https://www.w3.org/TR/webauthn-3/#sctn-credential-id-privacy-leak). Registration
  sends no exclusion list. Sign-in is discoverable. It sends an empty `allowCredentials`, the
  authenticator offers the passkey it holds, and the server finds that passkey by the returned
  credential ID. Offering every allowed credential, as first proposed, sent their IDs to anyone
  who started a sign-in.
- **Discoverable sign-in uses webauthn-rs's `conditional-ui` feature, with the browser's usual
  prompt.** The feature provides
  [`start_discoverable_authentication`](https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/struct.Webauthn.html#method.start_discoverable_authentication)
  and its finish. It also sets conditional mediation, which offers passkeys through autofill on a
  username field. The site has no such field, so the server clears the mediation, and a button
  opens the usual prompt.
- **Every passkey belongs to one account, `recording`, named for its purpose.** The account's
  user handle is a constant, and its name, `recording`, is what a password manager shows. A
  registration
  [overwrites](https://www.w3.org/TR/webauthn-3/#dom-publickeycredentialcreationoptions-user) any
  passkey the same authenticator holds for that handle, so registering again does not pile
  passkeys up in a password manager. Sign-in checks that the returned user handle is the
  account's, as [§7.2](https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion) requires.
  A fresh handle per registration lost: each registration added another passkey to the device.
- **Registration asks for a discoverable passkey.** Discoverable sign-in finds no other kind.
  webauthn-rs asks for
  [`residentKey`](https://www.w3.org/TR/webauthn-3/#dom-authenticatorselectioncriteria-residentkey)
  `discouraged`, and its verifier ignores the setting, so the server raises it to `required` in
  the options it sends. iCloud Keychain and 1Password create discoverable passkeys either way. A
  security key without discoverable storage then fails at registration, not at sign-in.
- **A ceremony waits under its own random ID for 10 minutes, and a finish takes it out.** The
  start answers with the ID and the options, and the finish sends the ID back. webauthn-rs never
  expires a ceremony itself, so the store's expiry does.
  [§13.4.3](https://www.w3.org/TR/webauthn-3/#sctn-cryptographic-challenges) recommends a
  challenge stay valid for about the upper limit of the recommended timeout range, 10 minutes. A
  browser can lengthen the 5-minute timeout it gets, for accessibility. Raised from 5 minutes on
  2026-10-13, after reading §13.4.3. Taking the ceremony out makes it single-use. Carrying it in
  the session, as first decided, gave every visitor who started a ceremony a 30-day session in
  redis.
- **Only a sign-in creates a session, and with a fresh ID.** No session exists before sign-in,
  so the
  [rule to renew the ID at sign-in](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#renew-the-session-id-after-any-privilege-level-change)
  holds by construction. The finish cycles the ID anyway, for a browser that signs in again.
  Registration creates no session.
- **The session cookie is `__Host-lookout-session`, `HttpOnly`, `Secure`, and `SameSite=Strict`.**
  A browser accepts a
  [`__Host-` cookie](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie#cookie_prefixes)
  only from a secure origin, without `Domain`, and with `Path=/`, so no other site can plant one.
  `fly.dev` is on the [Public Suffix List](https://publicsuffix.org/), which already keeps sibling
  fly apps from setting this site's cookies. The prefix holds wherever the site is hosted. Over
  plain HTTP the cookie is `lookout-session`, without the prefix or `Secure`.
- **The gate checks the passkey, not only the session.** A request to `/record/` or `/ws` needs a
  session that names a passkey still in `lookout-passkeys`. Revoking a passkey therefore
  ends its sessions at their next request. An open socket re-checks every minute and closes once
  its passkey is gone. Checking the session alone left a revoked phone signed in until its
  session expired.
- **`/ws` refuses an upgrade whose `Origin` differs from `LOOKOUT_ORIGIN`.** A browser sends
  `Origin` on every websocket handshake, and checking it is the
  [standard defence](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html#origin-header-validation)
  against cross-site websocket hijacking. `SameSite=Strict` already keeps the cookie off a
  cross-site upgrade in current browsers. The check holds without it.
- **`/signin/` always continues to `/record/`.** A `next` parameter is an
  [open redirect](https://cheatsheetseries.owasp.org/cheatsheets/Unvalidated_Redirects_and_Forwards_Cheat_Sheet.html)
  unless validated, and `/record/` is the only gated page.
- **`POST /auth/signout` ends the session.** It deletes the session's key and clears the cookie.
- **The stores sit behind traits, as the sample sink does.** One trait, `AuthStore`, covers
  ceremonies, pending passkeys, and allowed passkeys. Its memory implementation serves tests and
  runs without redis, and its redis implementation serves production. Sessions have the same
  pair: tower-sessions' `MemoryStore`, and `RedisSessionStore`.
- **Tests drive real ceremonies with 1Password's `passkey` crate.** It implements a client and an
  authenticator, with discoverable passkeys and user handles. The tests pass its credentials to
  the server as JSON, as a browser does. webauthn-rs's own `SoftPasskey` refuses resident keys and
  returns no user handle, so it cannot sign in discoverably.
- **The redis URL is the root of trust.** Whoever holds it can allow a passkey, as they can
  already push samples onto the queue. It lives in 1Password and in fly's secrets.
- **The browser creates the passkey, and the server stores its public key.** In WebAuthn the
  authenticator creates the key pair, and the private key never leaves it. The server issues the
  challenge and verifies what comes back.
- **The short code is 8 random Crockford base32 characters, minted at registration.** A
  credential ID is too long to read out. Eight characters give 2^40 codes, so a guessed or
  mistyped code matches no pending passkey. The store keys a pending passkey by its code. To
  revoke, the CLI scans `lookout-passkeys`, which holds a handful of entries.
- **Each sign-in writes the passkey back.** `finish_discoverable_authentication` returns a counter
  and backup state, and `Passkey::update_credential` applies them. Synced passkeys, such as
  1Password's, usually report a counter of 0, and webauthn-rs skips the counter check at 0. The
  write-back records the backup state and when the passkey was last used, not a counter for clone
  detection.
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
- **Invites before registration.** The admin would mint a code, and a registration would need
  that code, so a stranger would store nothing. The user kept the straw man's order on
  2026-10-13. If the hour's expiry on pending passkeys proves too loose a bound, invites come back.
- **A gate that turns off without redis.** Confirmed on 2026-10-12 and replaced on 2026-10-13. It
  failed open, and it derived an on/off switch from whether a config value was set, against the
  repo's Rust rules.
- **Ceremony state in the session.** Confirmed on 2026-10-12 and replaced on 2026-10-13. It gave
  every visitor who started a ceremony a 30-day session in redis.
- **Accounts for other purposes.** `recording` is the only account. If a later purpose needs
  access, it gets an account of its own, with its own user handle. Each passkey then records its
  account, and the gate checks the account a page needs.
- **A rate limit on ceremony starts.** Anyone can start a ceremony, which holds a few hundred
  bytes in redis for 10 minutes. If the redis bill or the store's size shows abuse, a limit comes
  in.

#### Open questions

- Whether iOS Safari has `PublicKeyCredential.parseCreationOptionsFromJSON` and `toJSON()`. If it
  has, `/signin/` passes the webauthn-rs JSON straight through. If not, the page converts
  base64url itself. The first sign-in from the phone answers it.
- How an iPhone reaches a server over HTTPS. WebAuthn needs a secure context, and a phone cannot
  use `localhost`. The localhost allowance covers only the laptop, so the phone's first sign-in
  runs against redis: a deploy to fly, or `just serve-redis` behind a tunnel.

### Observations

- **2026-11-04, registration against `just serve`.** Desktop Safari and Chrome each registered a
  passkey from the console. Each passed the server's options through
  `PublicKeyCredential.parseCreationOptionsFromJSON`, posted `toJSON()` back unchanged, and got
  `{"status": "allowed"}`. Registering again in Safari, 1Password offered to replace the
  `recording` passkey for `localhost`, and one passkey remained. The server still held the first
  passkey as allowed.

### Tasks

#### Refactors / extensions

- [x] Move `redis` to 1.7 across the workspace, and rewrite its TLS feature comment in `Cargo.toml`.
- [x] Run the Docker tests of `telemetry`, `server`, `recorder`, and `motis` against `redis` 1.7.
- [x] Move `tower-http` to 0.7, in a commit of its own.

#### A sign-in end to end, over memory stores

The ceremony is the largest unknown, so a sign-in runs end to end over the memory stores before
the redis stores exist. The memory stores stay, for tests and for running without redis.

- [x] Add `webauthn-rs` 0.5 with `danger-allow-state-serialisation`, and `tower-sessions`, to
      `crates/server`.
- [-] Add `/auth/register/start`, `/auth/register/finish`, `/auth/login/start`, and
      `/auth/login/finish`, over a `MemoryStore` and an in-memory passkey list. Superseded before
      commit by the security review.
- [x] Turn on webauthn-rs's `conditional-ui` feature, and add `rand` and `base32` for short codes,
      and `passkey` as a dev-dependency.
- [x] Add the `AuthStore` trait in `crates/server/src/auth/store.rs`, with its memory
      implementation and unit tests of expiry and single use.
- [x] Add `/auth/register/start` and `/auth/register/finish`, storing a pending passkey, or an
      allowed one for `localhost` without redis. Until the binary reads `LOOKOUT_ORIGIN`, its
      origin is `http://localhost:3000`, so a deployed server allows at once any passkey a script
      registers for `localhost`.
- [x] Add `/auth/signin/start` and `/auth/signin/finish`, creating the session.
- [ ] Add `/auth/signout`.
- [ ] Test the endpoints in `tests/auth.rs` with `passkey`, including that no answer before sign-in
      carries a credential ID.
- [ ] Add `static/signin/`, registering and signing in through `navigator.credentials`.
- [ ] Register and sign in from desktop Safari and Chrome against `just serve`.
- [-] Register and sign in from an iPhone over HTTPS. Moved to Wrap-up: a phone's origin is not
      `localhost`, so its passkey needs the admin, and the admin needs redis.

#### The redis stores

- [ ] Add the redis implementation of `AuthStore` in `crates/server/src/auth/redis.rs`.
- [ ] Test it against a containerised redis, as `redis_sink_docker.rs` tests the sink.
- [ ] Add `RedisSessionStore`, a `tower_sessions::SessionStore` over the `redis` client, with the
      session's expiry as its key's TTL.
- [ ] Test `RedisSessionStore` against a containerised redis.

#### The gate

- [ ] Choose the stores in `bin/server.rs`: redis with `LOOKOUT_REDIS_URL`, memory without, and log
      which.
- [-] Leave the gate off when `LOOKOUT_REDIS_URL` is unset, and warn at startup. Rejected on
      2026-10-13: the gate stays on, over memory stores.
- [-] Mark the session signed in when login finishes, and write the updated passkey back. Moved into
      the `/auth/signin/finish` task.
- [ ] Serve `/record/` through a layer that redirects a session without an allowed passkey to
      `/signin/`.
- [ ] Refuse a `/ws` upgrade whose session lacks an allowed passkey with 401, tested in
      `ws_roundtrip.rs`.
- [ ] Return 403 from a `/ws` upgrade whose `Origin` differs from `LOOKOUT_ORIGIN`.
- [ ] Re-check an open socket's passkey every minute. Once it is revoked, close the socket.
- [ ] Add `/auth/status`, returning whether the session is signed in.
- [ ] Read `LOOKOUT_ORIGIN` in `bin/server.rs`, and set it in `fly.toml`.

#### The admin CLI

- [ ] Add `crates/server/src/bin/passkeys.rs`, with clap subcommands `list`,
      `allow <code> [--label <name>]`, and `revoke <code>`.
- [ ] Add `just passkeys *args`, with the redis URL from 1Password.

#### The pages

- [ ] Show the short code on `/signin/` after registration, and tell the user to pass it to the
      admin.
- [ ] On each websocket close in `static/record/app.js`, check `/auth/status`. If signed out, show
      it with a link to `/signin/`.

#### Wrap-up

- [ ] Describe the gate, the session store, and the CLI in `crates/server/README.md`.
- [ ] Before `/complete-slice`, record in `crates/server/README.md` every fact about the server
      that only this slice doc holds, with its sources.
- [ ] Describe signing in, and the signed-out state of `/record/`, in `docs/web.md`.
- [ ] Describe who can record, how a passkey gains and loses access, and the risks the design
      accepts, in `docs/access.md`, and index it in `CLAUDE.md`.
- [ ] Run `cargo-mutants` over `crates/server/src/auth`, and add a test for each mutant that
      survives. Then review the auth code as a whole.
- [ ] Run `just test-no-docker` and the server's Docker tests.
- [ ] Deploy, register from the phone, allow it with `just passkeys allow`, and record a journey.
- [ ] Set 1Password as the iPhone's passkey provider, under AutoFill in Settings.
- [ ] Register through 1Password on the laptop, and allow it. Sign in from the iPhone without
      registering again.
- [ ] From a second browser, check that `/record/` redirects and that `/ws` refuses the upgrade.
- [ ] Revoke a passkey during a recording, and check that its socket closes within a minute.
