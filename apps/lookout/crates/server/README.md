# server

Serves the site and receives what a recording page sends: `/ws` for telemetry, `/version` for the
build's git hash, and everything else from the static directory. [The telemetry
wire](../../docs/telemetry.md) describes what arrives over the socket and when the server acks it;
[the browser](../../docs/web.md) describes the pages it serves.

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
`SampleSink`. A memory implementation of it holds them for the tests.

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
authenticator in one. They hand its credentials to webauthn-rs as JSON, the form a page posts.

[challenges]: https://www.w3.org/TR/webauthn-3/#sctn-cryptographic-challenges
[timeouts]: https://www.w3.org/TR/webauthn-3/#sctn-timeout-recommended-range
[default-timeout]: https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/constant.DEFAULT_AUTHENTICATOR_TIMEOUT.html
[crockford]: https://www.crockford.com/base32.html
[passkey]: https://docs.rs/passkey/0.6.0/passkey/

## Redis is optional, but a configured redis is not

With no queue configured the site still serves and logs what it receives, so a deploy is not gated
on redis. A queue that is configured and unreachable is fatal instead: running log-only would drop
every sample a phone sent while looking healthy. That failure prints to stderr, which ships
reliably even when the boot burst of tracing does not.

## The git hash comes from the build, not from `.git`

The Docker build context holds no `.git`, so the hash arrives as a build argument and is compiled
in, falling back to `unknown` for a bare `cargo build`. The server logs it at startup and serves
it at `/version`, which is how a reader matches a running deploy to its source.
