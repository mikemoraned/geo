# shared

The telemetry wire: the message a recording device sends, and the payloads it carries. The
format itself — the versioning rule, both message sets and what a reading holds — is
[the telemetry wire](../../docs/telemetry.md).

A message is a projection, so it keeps the short field names the archive was written with while
the types it carries name theirs in full. What a fix and a device are is
[`domain`](../domain/README.md); this crate adds the envelope and the sensor payloads with no
type of their own.
