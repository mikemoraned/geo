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
  runs against redis: a deploy to fly, or `just serve-local-redis` behind a tunnel, with
  `LOOKOUT_ORIGIN` set to the tunnel's URL.

### Observations

- **2026-11-04, registration against `just serve`.** Desktop Safari and Chrome each registered a
  passkey from the console. Each passed the server's options through
  `PublicKeyCredential.parseCreationOptionsFromJSON`, posted `toJSON()` back unchanged, and got
  `{"status": "allowed"}`. Registering again in Safari, 1Password offered to replace the
  `recording` passkey for `localhost`, and one passkey remained. The server still held the first
  passkey as allowed.
- **2026-11-05, `/signin/` against `just serve`.** Desktop Safari and Chrome each registered a
  passkey and signed in through the page, which continued to `/record/`.

### Investigation

**A revoke during a recording shows "reconnecting…" on the iPhone, not "signed out".** Seen in
both manual tests, the second on 2026-11-07 against `62f90663`, which has every fix under the
revoke task.

- The server closed the socket as signed out. It connected at 20:46:40 and disconnected at
  20:48:40, at the second minute's recheck, and logged no failed recheck.
- The page showed `gathering — reconnecting…` with `unsent samples: 0`. The page shows
  "reconnecting…" only when the close code is not 1008 and `/auth/status` does not answer
  `signed_out`, so both checks failed.
- The logs show nothing after the disconnect. Refused upgrades and status answers were not
  logged at INFO.
- Static files carry `no-cache` with an ETag that changes with each file's modification time, so a
  reload fetches the new `app.js`.

Two suspects. Fly's proxy can drop the close frame, so the browser sees 1006. Safari can still hold
a `signed_in` answer from before `/auth/status` sent `no-store`. A count of 0 does not fit either:
under `62f90663`, samples queue after the close, so the count climbs within 10 seconds.

Next: the status request now bypasses the browser's cache, and the server logs each refused
upgrade, each status answer, and each signed-out close. A revoke from a desktop browser, with the
Network tab open, shows the close code the browser receives.

**Cause: the iPhone runs an `app.js` stored before `28bf0dea`.** A third test on 2026-11-07,
against `489a7edc`, registered, signed in, revoked, and started gathering.

- The server logged the signed-out close at 21:17:35, then refused upgrades 1.2, 2.2, 4.2, 8.2,
  and 16.2 seconds apart, then every 30 seconds. That is the reconnect loop of the script before
  `343a2a26`. The deployed script reconnects after neither a 1008 close nor a signed-out status.
- The server logged no status answer. The deployed script requests `/auth/status` with
  `cache: "no-store"` before each reconnect, and that request reaches the server.
- The page showed `unsent samples: 0`. The line is in the new `index.html`, and only the new
  `app.js` updates it.

Before `28bf0dea`, `/record/app.js` carried no `Cache-Control`. Safari stored it with
[heuristic freshness](https://www.rfc-editor.org/rfc/rfc9111#name-calculating-heuristic-fresh),
reused it without asking the server, and so never received the `no-cache` the server now sends.
Safari revalidates a navigation, so `index.html` arrived new. The old script's status request went
to the same cache, so no status request reached the server, and a stored `signed_in` answer
explains "reconnecting…".

Clearing the site's data on the iPhone removes the stored script. Every file under `/record/` now
carries `no-cache`, so once replaced, the script stays current. Clearing also removes the outbox in
`localStorage` and the session cookie.

Confirmed on 2026-11-07: with the site's data cleared on the iPhone, a revoke during a recording
shows the session as signed out.

### Pending refactors

- [x] Check a stored session by building a fresh `Session` for its ID and asking `signed_in`,
      in place of the hand decode in `RelyingParty::stored_session_signed_in`. tower-sessions
      0.15's `Session::new(Some(id), store, None)` loads the record on first read, so the decode
      of `SIGNED_IN_PASSKEY` lives in one place, and `AuthError::SessionStore` goes. A missing
      record then logs tower-sessions' "record not found" warning once, at the closing recheck.
- [x] Inline `RelyingParty::allowed` into `signed_in`, its only caller after the refactor above.
- [x] Rename `PASSKEY_RECHECK` to `SESSION_RECHECK`, since the minute's recheck reads the session.
- [ ] Set `Cache-Control: no-store` on every answer that sets none, with one outer
      `SetResponseHeader::if_not_present` layer. Static files keep `no-cache`, and `/record/` keeps
      `private, no-cache`. The `/auth/` layer then goes, and its test stays. Three times a route
      sent no `Cache-Control`, and a browser reused a stored answer. With the default, a route
      that sets none is never stored.

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
- [x] Add `/auth/signout`.
- [x] Test the endpoints in `tests/auth.rs` with `passkey`, including that no answer before sign-in
      carries a credential ID.
- [x] Add `static/signin/`, registering and signing in through `navigator.credentials`.
- [x] Register and sign in from desktop Safari and Chrome against `just serve`.
- [-] Register and sign in from an iPhone over HTTPS. Moved to Wrap-up: a phone's origin is not
      `localhost`, so its passkey needs the admin, and the admin needs redis.

#### The redis stores

- [x] Add the redis implementation of `AuthStore` in `crates/server/src/auth/redis.rs`.
- [x] Test it against a containerised redis, as `redis_sink_docker.rs` tests the sink.
- [x] Add `RedisSessionStore`, a `tower_sessions::SessionStore` over the `redis` client, with the
      session's expiry as its key's TTL.
- [x] Test `RedisSessionStore` against a containerised redis.

#### The gate

- [x] Choose the stores in `bin/server.rs`: redis with `LOOKOUT_REDIS_URL`, memory without, and log
      which. Changed on 2026-11-05: `--store redis` or `--store in-memory` names the store, and the
      server refuses to start without one, so a lost redis secret cannot start production in memory.
- [-] Leave the gate off when `LOOKOUT_REDIS_URL` is unset, and warn at startup. Rejected on
      2026-10-13: the gate stays on, over memory stores.
- [-] Mark the session signed in when login finishes, and write the updated passkey back. Moved into
      the `/auth/signin/finish` task.
- [x] Serve `/record/` through a layer that redirects a session without an allowed passkey to
      `/signin/`.
- [x] Refuse a `/ws` upgrade whose session lacks an allowed passkey with 401, tested in
      `ws_roundtrip.rs`.
- [x] Return 403 from a `/ws` upgrade whose `Origin` differs from `LOOKOUT_ORIGIN`.
- [x] Re-check an open socket's passkey every minute. Once it is revoked, close the socket.
- [x] Add `/auth/status`, returning whether the session is signed in.
- [x] Read `LOOKOUT_ORIGIN` in `bin/server.rs`, and set it in `fly.toml`.

#### The admin CLI

- [x] Add `crates/server/src/bin/passkeys.rs`, with clap subcommands `list`,
      `allow <code> [--label <name>]`, and `revoke <code>`.
- [x] Add `just passkeys *args`, with the redis URL from 1Password.
- [x] Send `Cache-Control: private, no-cache` with every response the `/record/` gate lets
      through, tested on each `/record` path. Without it, Chrome showed a cached `/record/` to a
      signed-out browser until a force-refresh, since a response without `Cache-Control` is
      reused without asking the server.

#### The pages

- [x] Show the short code on `/signin/` after registration, and tell the user to pass it to the
      admin.
- [x] Send `Cache-Control: no-cache` with every static file, keeping `private, no-cache` on what
      the `/record/` gate lets through. Without it, Chrome ran a cached `signin.js` after the page
      changed, and a phone would keep each page's old script after a deploy.
- [x] On each websocket close in `static/record/app.js`, check `/auth/status`. If signed out, show
      it with a link to `/signin/`.
- [x] Close an open socket once its session signs out, at the same minute's recheck as a revoked
      passkey. The recheck reads only the passkey, so a recording kept sending after
      `POST /auth/signout` until its passkey was revoked or the socket dropped.

#### End to end manual test

A fresh deploy to production, tested from the laptop's browsers and the iPhone. It runs before
Wrap-up, since what it finds can change what the docs describe. The phone needs HTTPS, and a
passkey on a non-`localhost` origin needs the admin, so this phase is the first to run against
Upstash.

- [x] In the Upstash console, check that eviction is off. Until this slice, redis held only queued
      samples. `lookout-passkeys` has no expiry, and eviction under memory pressure would delete it,
      so every device would need to register again.
- [x] Run `just push-secrets`, and check with `fly secrets list` that `LOOKOUT_REDIS_URL` is set.
      Under `--store redis`, the server refuses to start without it.
- [x] Deploy, and check that `/version` gives the deployed commit's hash.
- [x] From desktop Safari, register on `/signin/`, allow the code with
      `just passkeys allow <code> --label <name>`, and sign in. Check that `/signin/` continues to
      `/record/`.
- [x] Check that `just passkeys list` shows the label, and a last use after the sign-in.
- [x] From a second browser, check that `/record/` redirects and that `/ws` refuses the upgrade.
- [x] Register from the iPhone, allow it with `just passkeys allow`, and record a journey. Answer the
      open question on `parseCreationOptionsFromJSON` and `toJSON()` in iOS Safari.
- [x] Set 1Password as the iPhone's passkey provider, under AutoFill in Settings.
  - was already set
- [x] Register through 1Password on the laptop, and allow it. Sign in from the iPhone without
      registering again.
    - did this but the default passkey suggested on iPhone didn't work but the option available when I did "More options" and used 1Password worked (this was the Passkey I had created on the laptop)
- [x] Revoke a passkey during a recording, and check that its socket closes within a minute, and
      that `/record/` then shows the session as signed out.
    - this was a bit unclear: I revoked the passkey and it seemed to go to "gathering — reconnecting…" at some point but not sure what this means
    - when I did a reload of the /record page, it did then redirect me to sign-in
    - so, maybe it was working, but UI needs to display it better?
- [x] review and improve how revoking during a /record shows in UI
  - [x] Show a socket the server closes with 1008 as signed out, from the close event alone.
        `/record/` ignores the close code and asks `/auth/status`, so a failed status request shows
        a revoke as "reconnecting…". Keep the status request for a refused reconnect, where the
        browser hides the reason.
  - [x] Send `Cache-Control: no-store` with every `/auth/` response, tested. `/auth/status` sends
        no `Cache-Control`, so a browser can answer it from its cache.
  - [x] Stop reconnecting once signed out, and try again when the page next becomes visible. Each
        retry now gets a 401 and sends another status request, every 30 seconds, without end.
  - [x] Show in the status how many samples wait on the device, so a signed-out or disconnected
        recording shows that its samples are kept. The count has a line of its own,
        `unsent samples`, under the status. The status changes on each connect and close, and the
        count on each sample and ack.
  - [x] Say in the signed-out text that signing in ends this recording, and that the waiting
        samples go once `/record/` is open again. The link stays in the same tab, since two tabs
        would share one outbox.
- [x] Revoke during a recording from a desktop browser with the Network tab open, and from the
      iPhone, and check the close code, the status answer, and the server's logs. The iPhone still
      showed "reconnecting…" after the fixes above. See Investigation.
      Only the iPhone ran it. The server's logs found the cause, a stored `app.js`, without the
      desktop browser's close code.

#### Harden

Checks that the gate holds beyond the cases its tests name. It runs after the manual test, so a
fix the manual test forces is hardened too, and before Wrap-up, so the docs describe the hardened
code.

- [ ] Run `cargo-mutants` over `crates/server/src/auth`, and add a test for each mutant that
      survives.
- [ ] Review the auth code as a whole, with `/security-review` over the slice's changes.
- [ ] Run `cargo audit` over the workspace, for advisories against the crates this slice adds:
      webauthn-rs, its vendored OpenSSL, tower-sessions, and their dependencies.
- [ ] Check what an error tells a caller. A 400 carries webauthn-rs's reason for refusing a
      credential. Decide whether a caller needs it, or only the server's log.
- [ ] Check that no log line carries a session ID, a credential ID, or the redis URL. Include the
      redis client's errors, which the server and the CLI print.
- [ ] Run `just test-no-docker` and the server's Docker tests.

#### Wrap-up

- [x] Describe the gate, the session store, and the CLI in `crates/server/README.md`.
- [ ] Before `/complete-slice`, record in `crates/server/README.md` every fact about the server
      that only this slice doc holds, with its sources.
- [ ] Rewrite the capture paragraph of `docs/architecture.md`, which says the server runs without
      redis by logging samples. The server now takes `--store redis` or `--store in-memory`, and
      redis also holds passkeys and sessions.
- [ ] Describe signing in, and the signed-out state of `/record/`, in `docs/web.md`.
- [ ] Describe who can record, how a passkey gains and loses access, and the risks the design
      accepts, in `docs/access.md`, and index it in `CLAUDE.md`.
