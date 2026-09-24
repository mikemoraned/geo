# The telemetry wire

The format a recording device sends and the archive keeps. What the samples become once
drained is [architecture.md](architecture.md); the store that holds them is
[medallion.md](medallion.md).

## A version in every message, and absent means 0

Each message is a JSON object carrying its protocol version in a top-level `v`. A message
without one is version 0, which is what the unversioned payloads already in the archive read
as. Raw payloads are re-interpreted from the archive rather than migrated, so both versions
stay readable and an unknown version is refused.

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
