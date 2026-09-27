# shared

The message a recording device sends, and the payloads it carries.
[The telemetry wire](../../docs/telemetry.md) carries the format itself: the versioning rule, both
message sets, and what a reading holds.

A message is a projection, so it keeps the short field names the archive was written with while the
types it carries name theirs in full. [`domain`](../domain/README.md) describes what a fix and a
device are; this crate adds the envelope, and the sensor payloads nothing else describes.
