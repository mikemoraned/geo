# server

Serves the site and receives what a recording page sends: `/ws` for telemetry, `/version` for the
build's git hash, and everything else from the static directory.
[The telemetry wire](../../docs/telemetry.md) describes what arrives over the socket and when the
server acks it; [the browser](../../docs/web.md) describes the pages it serves.

## A page is a directory

The server serves the static directory as it finds it, so `/live` is `live/index.html` and adding a
page means adding a directory rather than a route in the binary. It compresses every response, which
the crossings — 180 KB of coordinates — are the first to need and the rest get for free.

## The queue is a port, so tests drive the real handler

The handler pushes to a sink rather than to redis, so a test can swap in a recording sink and
exercise the real websocket path without a container. The production sink pushes onto the shared
queue, whose key and connection belong to [`telemetry`](../telemetry/README.md).

A connection handle is cheap to clone, so each push clones one rather than locking a shared
connection between concurrent sockets.

## Redis is optional, but a configured redis is not

With no queue configured the site still serves and logs what it receives, so a deploy is not gated
on redis. A queue that is configured and unreachable is fatal instead: running log-only would drop
every sample a phone sent while looking healthy. That failure prints to stderr, which ships reliably
even when the boot burst of tracing does not.

## The git hash comes from the build, not from `.git`

The Docker build context holds no `.git`, so the hash arrives as a build argument and is compiled
in, falling back to `unknown` for a bare `cargo build`. The server logs it at startup and serves it
at `/version`, which is how a reader matches a running deploy to its source.
