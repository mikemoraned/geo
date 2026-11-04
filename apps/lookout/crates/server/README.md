# server

Serves the site and receives what its pages send. `/ws` takes telemetry, `/auth/` registers
passkeys, `/version` gives the build's git hash, and the static directory serves everything else.
[The telemetry wire](../../docs/telemetry.md) describes what arrives over the socket and when the
server acks it; [the browser](../../docs/web.md) describes the pages it serves.

## A page is a directory

The server serves the static directory as it finds it, so `/live` is `live/index.html` and adding
a page means adding a directory rather than a route in the binary. It compresses every response,
which the crossings — 180 KB of coordinates — are the first to need and the rest get for free.

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
`SampleSink`. A memory implementation of it holds them for the tests and for the server.

Taking a ceremony or a pending passkey removes it, so each is used at most once. The server holds
a challenge until its ceremony completes, as [§13.4.3 of WebAuthn][challenges] recommends.
§13.4.3 also recommends a challenge stay valid for about the upper limit of the [recommended
timeout range][timeouts], so a ceremony lasts 10 minutes. The browser gets the [5-minute
default][default-timeout], which it can lengthen for accessibility. webauthn-rs records no time in
a ceremony, so the store expires it.

A pending passkey waits an hour under its short code. No two pending passkeys hold the same code,
so allowing a code allows exactly one passkey. Allowing moves the passkey into the allowed
passkeys, which stay until revoked.

A short code is 8 characters of [Crockford's base32][crockford], 40 random bits, shown in two
groups of four as `K7Q2-M9XD`. Reading a typed code follows Crockford's decoding rules: either
case, hyphens ignored, `O` read as `0`, and `I` and `L` read as `1`. The server also ignores
spaces.

Tests register real passkeys through [1Password's `passkey` crate][passkey], a client and an
authenticator in one. They hand its credentials to webauthn-rs as JSON, the form a page posts. The
crate's [`MemoryStore`][memory-store] finds a passkey only by a credential ID it is given. A store
of one passkey, `Option<Passkey>`, [finds it with none given][option-store], as a discoverable
sign-in needs. The endpoint tests give each browser that store.

[challenges]: https://www.w3.org/TR/webauthn-3/#sctn-cryptographic-challenges
[timeouts]: https://www.w3.org/TR/webauthn-3/#sctn-timeout-recommended-range
[default-timeout]: https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/constant.DEFAULT_AUTHENTICATOR_TIMEOUT.html
[crockford]: https://www.crockford.com/base32.html
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

## Redis is optional, but a configured redis is not

With no queue configured the site still serves and logs what it receives, so a deploy is not gated
on redis. A queue that is configured and unreachable is fatal instead: running log-only would drop
every sample a phone sent while looking healthy. That failure prints to stderr, which ships
reliably even when the boot burst of tracing does not.

## The git hash comes from the build, not from `.git`

The Docker build context holds no `.git`, so the hash arrives as a build argument and is compiled
in, falling back to `unknown` for a bare `cargo build`. The server logs it at startup and serves
it at `/version`, which is how a reader matches a running deploy to its source.
