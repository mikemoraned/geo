# server

Serves the site and receives what its pages send. `/ws` takes telemetry, `/record/` serves the
recording page to a signed-in browser, `/auth/` registers passkeys and signs them in, `/version`
gives the build's git hash, and the static directory serves everything else.
[The telemetry wire](../../docs/telemetry.md) describes what arrives over the socket and when the
server acks it; [the browser](../../docs/web.md) describes the pages it serves.

## A page is a directory

The server serves the static directory as it finds it, so `/live` is `live/index.html` and adding
a page means adding a directory rather than a route in the binary. It compresses every response,
which the crossings — 180 KB of coordinates — are the first to need and the rest get for free.

Every static file goes out with `Cache-Control: no-cache`, so a browser [revalidates][no-cache] a
stored copy with the server before reusing it. A page therefore loads its current script after a
deploy. A response without `Cache-Control` is [heuristically fresh][heuristic], and a browser
reuses it without asking.

A response whose route sets no `Cache-Control` gets `no-store` from the server's outermost layer,
so a browser [stores none of it][no-store]. A route that names no caching rule therefore never
leaves a stored copy behind.

[no-cache]: https://www.rfc-editor.org/rfc/rfc9111#name-no-cache-2
[heuristic]: https://www.rfc-editor.org/rfc/rfc9111#name-calculating-heuristic-fresh
[no-store]: https://www.rfc-editor.org/rfc/rfc9111#name-no-store-2

## The queue sits behind a trait, so tests drive the real handler

The handler pushes to a `SampleSink` rather than to redis, so a test can swap in a recording sink
and exercise the real websocket path without a container. The production sink pushes onto the
shared queue, whose key and connection belong to [`telemetry`](../telemetry/README.md).

A connection handle is cheap to clone, so each push clones one rather than locking a shared
connection between concurrent sockets.

## Sign-in keeps its records behind a trait

Sign-in keeps three kinds of record. A ceremony is the server's half of a registration or a
sign-in: the challenge it issued. A pending passkey has been registered but not yet allowed. An
allowed passkey can sign in. All three sit behind one trait, `AuthStore`, as samples sit behind
`SampleSink`. A memory implementation of it holds them for the tests and for the server, and a
redis implementation keeps them in redis.

Taking a ceremony or a pending passkey removes it, so each is used at most once. The server holds
a challenge until its ceremony completes, as [§13.4.3 of WebAuthn][challenges] recommends.
§13.4.3 also recommends a challenge stay valid for about the upper limit of the [recommended
timeout range][timeouts], so a ceremony lasts 10 minutes. The browser gets the [5-minute
default][default-timeout], which it can lengthen for accessibility. webauthn-rs records no time in
a ceremony, so the store expires it.

A pending passkey waits an hour under its short code. No two pending passkeys hold the same code,
so allowing a code allows exactly one passkey. Allowing moves the passkey into the allowed
passkeys, which stay until revoked.

A registration draws a fresh short code for its pending passkey, and a sign-in draws a fresh
session ID. Each stores its record under the drawn ID only if nothing holds that ID. If something
does, the server waits and draws again, through [backon][backon]: a second at first, doubling
each time. Once the next wait would take the total past 10 seconds, it [stops][total-delay] and
the request fails with 500. A store in which every ID is taken, as a broken key would make it,
then fails the request rather than holding it open.

A short code is 8 characters of [Crockford's base32][crockford], 40 random bits, shown in two
groups of four as `K7Q2-M9XD`. Reading a typed code follows Crockford's decoding rules: either
case, hyphens ignored, `O` read as `0`, and `I` and `L` read as `1`. The server also ignores
spaces.

In redis, each record is JSON. A ceremony sits under `lookout-ceremony:<id>`, and a pending passkey
under `lookout-passkey-pending:<code>`. Each key expires with its record. [`GETDEL`][getdel] takes
a record out, so two finishes cannot both take one ceremony. A pending passkey is written with
[`SET NX`][set], which refuses a code that another pending passkey holds. The allowed passkeys
share one hash, `lookout-passkeys`, keyed by the credential ID's bytes. A sign-in writes its
passkey back through a script, which replaces the passkey only if the hash still holds it. Redis
[blocks every other command][eval] while a script runs, so a revocation cannot fall between the
check and the write.

Tests register real passkeys through [1Password's `passkey` crate][passkey], a client and an
authenticator in one. They hand its credentials to webauthn-rs as JSON, the form a page posts. The
crate's [`MemoryStore`][memory-store] finds a passkey only by a credential ID it is given. A store
of one passkey, `Option<Passkey>`, [finds it with none given][option-store], as a discoverable
sign-in needs. The endpoint tests give each browser that store. Both kinds of test build their
authenticator through [`test-passkeys`](../test-passkeys/README.md).

[challenges]: https://www.w3.org/TR/webauthn-3/#sctn-cryptographic-challenges
[timeouts]: https://www.w3.org/TR/webauthn-3/#sctn-timeout-recommended-range
[default-timeout]: https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/constant.DEFAULT_AUTHENTICATOR_TIMEOUT.html
[crockford]: https://www.crockford.com/base32.html
[backon]: https://docs.rs/backon/1.6.0/backon/
[total-delay]: https://docs.rs/backon/1.6.0/backon/struct.ExponentialBuilder.html#method.with_total_delay
[getdel]: https://redis.io/docs/latest/commands/getdel/
[set]: https://redis.io/docs/latest/commands/set/
[eval]: https://redis.io/docs/latest/develop/programmability/eval-intro/
[passkey]: https://docs.rs/passkey/0.6.0/passkey/
[memory-store]: https://docs.rs/passkey-authenticator/0.6.0/src/passkey_authenticator/credential_store.rs.html#90
[option-store]: https://docs.rs/passkey-authenticator/0.6.0/src/passkey_authenticator/credential_store.rs.html#136

## Anyone can register a passkey, and the admin allows it

Registration takes two requests, and neither needs a session. `POST /auth/register/start` answers
with a ceremony ID and the options for [`navigator.credentials.create()`][create].
`POST /auth/register/finish` takes back the ceremony ID with the credential the browser created.
The server verifies the credential against the ceremony and keeps the passkey's public key.

Every passkey belongs to one account, `recording`, and a password manager shows that name. The
account's [user handle][user] is a constant. An authenticator overwrites a discoverable passkey
with the same site and user handle, so registering again on a device replaces its passkey.

The options ask for a [discoverable passkey][resident-key], which an authenticator finds without
being given its credential ID. webauthn-rs starts a passkey registration [with
`residentKey: discouraged`][discouraged], and its verifier [ignores the setting][ignored]. The
server raises it to `required` in the options it sends. An authenticator that cannot store a
discoverable passkey then fails at registration, not at its first sign-in.

The options carry no exclusion list. Anyone can start a registration, so an exclusion list would
show every caller the server's credential IDs. [§14.6.3 of WebAuthn][id-leak] describes the same
leak through a sign-in's allow list.

A verified registration leaves a pending passkey under a fresh short code. The finish answers
`{"status": "pending", "code": "K7Q2M9XD"}`, and the admin allows the passkey by that code. A
server that keeps its records in memory allows a passkey registered for `localhost` at once, since
no admin can reach that memory. The finish then answers `{"status": "allowed"}`. Any other
origin's passkey stays pending, so a hosted server with its records in memory allows none.

A finish answers 400 for a ceremony that expired, finished already, or never started. It also
answers 400 for a credential that fails verification. The finish takes its ceremony out before
verifying, so a failed finish uses the ceremony up. A failure to start a ceremony, or to read or
write the records, answers 500.

[create]: https://www.w3.org/TR/webauthn-3/#sctn-createCredential
[user]: https://www.w3.org/TR/webauthn-3/#dom-publickeycredentialcreationoptions-user
[resident-key]: https://www.w3.org/TR/webauthn-3/#dom-authenticatorselectioncriteria-residentkey
[discouraged]: https://docs.rs/webauthn-rs/0.5.5/src/webauthn_rs/lib.rs.html#566
[ignored]: https://docs.rs/webauthn-rs-core/0.5.5/src/webauthn_rs_core/core.rs.html#393
[id-leak]: https://www.w3.org/TR/webauthn-3/#sctn-credential-id-privacy-leak

## A sign-in names no passkey, and leaves a session

Sign-in takes two requests, as registration does. `POST /auth/signin/start` answers with a ceremony
ID and the options for [`navigator.credentials.get()`][get]. The options carry an empty allow list,
so the authenticator offers the passkey it holds for the site, and no caller learns a credential ID.
webauthn-rs asks for [conditional mediation][conditional-set], which offers passkeys [through
autofill][conditional] on a username field. The site has no such field, so the server removes the
mediation, and the browser shows its usual prompt.

`POST /auth/signin/finish` takes back the ceremony ID with the browser's assertion. The server reads
the user handle and the credential ID from the assertion. It refuses with 403 a user handle other
than the `recording` account's, as [§7.2 of WebAuthn][verifying] requires. It also refuses with 403
a credential ID that no allowed passkey holds. It then verifies the assertion against that passkey's
public key.

A verified sign-in writes the passkey back with its [updated counter and backup state][update] and
the time of its use. The write replaces only a passkey that is still allowed. A passkey the admin
revokes during a sign-in therefore stays revoked, and the finish answers 403. Otherwise the finish
gives the session a [fresh ID][renew], records the passkey's credential ID in it, and answers 204.
Registration creates no session.

Over HTTPS the session cookie is `__Host-lookout-session`, with `Secure`. A browser accepts a
[`__Host-` cookie][host] only from a secure origin, so over plain HTTP the cookie is
`lookout-session`, without `Secure`. Both are `HttpOnly` and `SameSite=Strict`, with `Path=/`. A
session ends 30 days after its last use, and [every request that carries it][always-save] renews it.

A finish answers 400 as a registration's does: for a ceremony that expired, finished already, or
never started, and for an assertion that fails verification.

`POST /auth/signout` deletes the session's record and clears its cookie. It answers 204 whether or
not the request carries a session.

[get]: https://www.w3.org/TR/webauthn-3/#sctn-getAssertion
[conditional-set]: https://docs.rs/webauthn-rs/0.5.5/src/webauthn_rs/lib.rs.html#1341
[conditional]: https://w3c.github.io/webappsec-credential-management/#dom-credentialmediationrequirement-conditional
[verifying]: https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion
[update]: https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/prelude/struct.Passkey.html#method.update_credential
[renew]: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#renew-the-session-id-after-any-privilege-level-change
[host]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie#cookie_prefixes
[always-save]: https://docs.rs/tower-sessions/0.15.0/tower_sessions/service/struct.SessionManagerLayer.html#method.with_always_save

## The gate checks the session's passkey on every request

`/record/` and `/ws` need a signed-in session, and the other pages stay open. A session is signed
in while the passkey it names is still allowed. The gate reads the credential ID from the session
and looks it up in the allowed passkeys, so revoking a passkey ends its sessions at their next
request.

A request for `/record/` without a signed-in session redirects to `/signin/`. A signed-in request
gets `Cache-Control: private, no-cache` in place of the static files' `no-cache`. [`private`][private]
keeps the page out of shared caches, and `no-cache` makes a browser ask the server before showing a
stored copy. A browser that has signed out therefore gets the redirect, not its stored page.

The page's files sit in `gated/record/`, outside `static/`, which the server serves to anyone.
[`ServeDir`][serve-dir] reads `//record/` as `record/`, and `/record%2Fapp.js` as `record/app.js`.
Neither path matches the `/record` route, so each request goes to `static/`. Outside `static/`,
the page is reachable only through the gate.

`/ws` checks the `Origin` header before the session. It refuses with 403 an upgrade whose `Origin`
differs from the server's origin, `LOOKOUT_ORIGIN`. Checking `Origin` is the [standard
defence][ws-origin] against cross-site websocket hijacking. `SameSite=Strict` already keeps the
cookie off a cross-site upgrade in current browsers, and the check holds without it. An upgrade
without a signed-in session gets 401.

An open socket rechecks its session every minute. The upgrade request has finished by then, so the
recheck loads the session from the store by its ID. If the session has signed out, or its passkey
has been revoked, the server closes the socket with [1008, policy violation][close-codes]. If the
recheck fails, the server closes the socket with 1011, internal error, rather than let a recording
continue unchecked.

`GET /auth/status` answers `{"status": "signed_in"}` or `{"status": "signed_out"}`, by the gate's
check. A page uses it to tell a signed-out session from a dropped connection.

No route under `/auth/` sets `Cache-Control`, so every answer there carries the server's default,
`no-store`. A stored status would show a revoked session as signed in.

[private]: https://www.rfc-editor.org/rfc/rfc9111#name-private
[serve-dir]: https://docs.rs/tower-http/0.7.1/tower_http/services/struct.ServeDir.html
[ws-origin]: https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html#origin-header-validation
[close-codes]: https://www.rfc-editor.org/rfc/rfc6455#section-7.4.1

## A session in redis expires with its key

tower-sessions handles the cookie and its renewal, and a [`SessionStore`][session-store] holds the
records. The server has two: tower-sessions' `MemoryStore`, and `RedisSessionStore`, which shares
the queue's redis connection.

`RedisSessionStore` writes a session as JSON under `lookout-session:<id>`. Each write sets the key
to expire with the session, through [`SET EXAT`][set], so redis removes a session 30 days after its
last use without a sweep. Every request that carries a session renews it, so each such request
writes to redis once.

Creating a session writes with `NX`. While another session holds the ID, the store draws a fresh
one, within the 10-second limit on drawing a fresh ID. tower-sessions
[leaves a collision][create-session] to each store, which can draw again or fail.

[session-store]: https://docs.rs/tower-sessions/0.15.0/tower_sessions/trait.SessionStore.html
[create-session]: https://docs.rs/tower-sessions/0.15.0/tower_sessions/trait.SessionStore.html#method.create

## The admin allows and revokes passkeys from the command line

The `passkeys` binary lists, allows, and revokes passkeys:

```
passkeys list
passkeys allow <code> [--label <name>]
passkeys revoke <code>
```

It reads `LOOKOUT_REDIS_URL` and works on redis alone, since no command can reach a server's
memory. Whoever holds the redis URL can allow a passkey, as they can already push samples onto the
queue.

`allow` moves the pending passkey with that code into the allowed passkeys. The label tells devices
apart in the list. `allow` fails for a code that expired or never existed. `list` shows each
allowed passkey's code, label, when it was allowed, and when it last signed in. `revoke` removes
the allowed passkey with that code. The gate then refuses its sessions at their next request, and
an open socket closes within a minute.

An allowed passkey keeps the code it was registered under, and `revoke` takes the code `list`
shows. The allowed passkeys are keyed by credential ID, so `revoke` reads the whole hash to find
the code. The hash holds one passkey per device, few enough to read whole.

`just passkeys` runs the binary with the production redis URL from 1Password. `just passkeys-local`
runs it against the redis that `just local-redis` starts.

## The store is named at startup

The server takes `--store redis` or `--store in-memory`, and starts with neither unnamed. A store
chosen by whether `LOOKOUT_REDIS_URL` happened to be set would let a production server lose its
secret and start quietly in memory, forgetting every passkey and session on restart. `redis` needs
`LOOKOUT_REDIS_URL`, and refuses to start without it. `in-memory` ignores the URL, and logs a
warning when it is set.

With `redis`, the queue, the sign-in records, and the sessions share one connection, and every
passkey waits for the admin, `localhost`'s included. A redis that is unreachable at startup is
fatal. That failure prints to stderr, which ships reliably even when the boot burst of tracing does
not.

With `in-memory`, the server logs the samples it receives rather than queuing them, and keeps
passkeys and sessions in memory.

## The git hash comes from the build, not from `.git`

The Docker build context holds no `.git`, so the hash arrives as a build argument and is compiled
in, falling back to `unknown` for a bare `cargo build`. The server logs it at startup and serves
it at `/version`, which is how a reader matches a running deploy to its source.
