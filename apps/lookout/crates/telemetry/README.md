# telemetry

The queue a sample waits on between being received and being archived: its key, how to reach it,
and how to read an item back off it. Both sides of the queue depend on this crate rather than
writing the contract out twice. Pushing belongs to whoever receives samples, and lives there.

What the queue holds and in which order is [the telemetry wire](../../docs/telemetry.md).
