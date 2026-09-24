# telemetry

Where a sample waits between arriving and reaching the archive: the queue's key, how to reach it,
and how to read an item back off it. Both sides of the queue depend on this crate rather than
writing the contract out twice. Pushing belongs to whoever receives samples, and lives there.

[The telemetry wire](../../docs/telemetry.md) describes what the queue holds, and in which
order.
