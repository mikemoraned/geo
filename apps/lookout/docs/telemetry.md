# The telemetry wire

The format a recording device sends and the archive keeps.
[architecture.md](architecture.md) covers what the samples become once drained;
[medallion.md](medallion.md) covers the store that holds them.

## A version in every message, and absent means 0

Each message is a JSON object carrying its protocol version in a top-level `v`. A message without
one is version 0, which is how the archive's unversioned payloads read. Nothing migrates a raw
payload — it is re-interpreted from the archive — so both versions stay readable, and an unknown
version is refused.

Version 0 carries no message type: the variant is whichever sensor key is present.

```json
{"id":"…","t":1700000000000,"gps":{"lat":…,"lon":…,"alt":null,"acc":…}}
{"id":"…","t":1700000000000,"accel":{"x":…,"y":…,"z":…}}
```

Version 1 carries a `type`, which admits a variant that no sensor key would name — a device
announcing itself as a session starts.

```json
{"v":1,"type":"start_session","id":"…","t":…,"device":{…}}
{"v":1,"type":"gps","id":"…","t":…,"gps":{…}}
{"v":1,"type":"acceleration","id":"…","t":…,"accel":{…}}
```

A recording page sends version 1, and nothing writes version 0 any more. The archive holds
both: a minority of unversioned payloads, sent before the field existed, among versioned ones
from every ingest since. Reading a payload back therefore has to accept either shape.

## Every message names a device and an instant

`id` is the device, not the message: a UUID the browser mints once and keeps in a cookie, so
the samples of one phone share it across sessions. `t` is epoch milliseconds, and for a fix it
is the instant the receiver reported rather than the instant the sample was sent — the two are
up to a sampling interval apart.

Both keep their short names because the archive holds them that way. The fields of a fix do
too: a message's `gps` is the projection of a reported fix onto the names it was first sent
under.

## An accelerometer reading is a window, not an instant

A sample goes out every 10 seconds and the accelerometer reports at around 60 Hz, so an
instantaneous reading at that rate would measure gravity and nothing else. Each message instead
carries the window between samples, reduced from the gravity-removed magnitude:

- `rms` — ride roughness.
- `peak` — jolts and pointwork.
- `n` — how many readings the window aggregated, which shows whether it was sampled at all or
  the page was suspended.

A single raw reading (`x`, `y`, `z`, gravity removed) rides alongside for a tilt view. Version 0
carried only that reading, so the aggregates default to zero rather than failing to parse.

## The queue between sending and keeping

A sample arriving over a websocket is pushed onto a redis list, and a later run drains the list
into the archive. The list is one key, `lookout-telemetry`, and an item on it is the payload
verbatim beside `received_at` — the epoch millis of its receipt.

That stamp is taken at receipt rather than at the drain, so queue latency does not distort it,
and it is not the `t` inside the payload, which is the device's own clock and drifts. It rides
beside the payload rather than inside it, so the payload — and the md5 the archive keys on —
stay exactly what was sent. Nothing parses a payload on the way through, so one that no version
can interpret is still archived.

Samples are pushed at the head and taken from the tail, oldest first. Putting one back therefore
puts it on the tail again, ahead of the newer samples, so a failed archive loses nothing. Reading
without removing starts at the head instead, newest first.

The endpoint is TLS-only, so the URL is `rediss://` and the process negotiating it installs a
rustls crypto provider at startup. Redis's own timeouts — a second to connect, half of one to
answer — are too tight for a public-internet endpoint, so the timeouts here are generous enough to
cross it and still bound a hang. A fractional blocking-pop timeout needs redis 6 or newer, so the
test against a containerised queue pins an image new enough, as `FRACTIONAL_TIMEOUT_TAG`; the
testcontainers module's own default is older.

## An ack is what lets a sample be forgotten

A sample goes into an outbox before it goes over the socket, and leaves the outbox only once the
server has acked it. The server acks after it has taken responsibility — the sample is queued, or
there is no queue configured and it was logged — so a page reload or a mid-flush disconnect
re-sends the un-acked tail rather than losing samples that looked sent. A transient failure to
queue withholds the ack, which is what gets that sample retried.

The server acks malformed JSON and discards it. Re-sending cannot fix it, and withholding the ack
would block the outbox behind a message that can never succeed.

One ack retires one sample: delivery over a single socket is in order, so the frame's content
carries nothing and each ack retires the oldest sample in flight. Re-sending a sample twice costs
nothing either, since a drain deduplicates on the device and the instant.

A page persists its outbox, so one reloaded mid-journey still delivers what it captured. Where a
browser refuses to store it — quota, or private browsing — the page keeps capturing in memory rather
than failing the recording.
