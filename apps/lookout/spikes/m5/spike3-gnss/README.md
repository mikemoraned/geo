# Spike 3 — GPS in

Reads NMEA from the GPS/BDS Unit v1.1 (AT6668) on the Grove port, parses it into a
position, and renders time + lat/lon on the screen. Builds on spike 2's core/shell split.

```sh
just test      # run the core's tests on the laptop
just flash     # build, flash, and tail the serial console
just capture   # save sentences from a real fix to a file (needs sky view)
```

## Wiring

| | |
|---|---|
| Grove port | G32 / G33 |
| Baud | 115200 8N1, NMEA 0183 4.1 |
| UART | **never UART0** — that is the USB console |

The receiver's TX goes to the Stick's RX. Which of G32/G33 that is depends on who you ask,
and the wrong choice is indistinguishable from a dead receiver — so the shell doesn't
choose. It opens **both** pins at once on separate UARTs (G32 on UART1, G33 on UART2),
listens for three seconds, and keeps whichever one carries NMEA:

```
probed G32: 0 bytes, NMEA: false
probed G33: 412 bytes, NMEA: true
GNSS on G33 at 115200 baud
```

The two are electrically independent, so listening on the idle one costs nothing. If
neither sees NMEA the shell exits with a message rather than showing an empty screen —
that means the unit is unplugged, unpowered, or at a different baud rate.

## Parsing lives in the core

The straw man put the `nmea` parse in the shell. It is in the core instead, because a GPS
needs sky view and a ~23s cold start — deskbound iteration is the norm, and that only works
if the parsing is testable on the laptop. The core's tests cover a GGA fix, an RMC fix, a
fix moving, a "void" (no-fix) RMC leaving the last position alone, a corrupt checksum being
ignored, and line noise not panicking.

The shell is left with what only it can do: bytes off a UART, assembled into lines.

### The fixtures come from real captures, with the position removed

The sentence shapes are as the AT6668 emits them, captured via `just capture`. That mattered:
the first version of these fixtures was hand-written from NMEA 0183 examples and **got the
`RMC` field count wrong** — the real receiver speaks NMEA 4.1 and appends a
mode/navigational-status pair the older format has no room for.

The no-fix fixtures are byte-exact captures. The fix-carrying ones have **their coordinates
replaced**, because a real fix records where and when someone was, and a run of them is a
movement trace — not something to commit. Since changing the digits invalidates the captured
checksum, the tests recompute it (`sentence()`), which also documents the checksum rule.

Raw captures are gitignored (`*.nmea`) for the same reason.

## Notes

- The `nmea` crate's `Nmea` accumulator is held in the core's model. Sentences each carry
  part of the picture, so the parser has to keep state across them, and a fix is only
  promoted once both a latitude and a longitude are known.
- Multi-constellation receivers emit `$GN*` sentences rather than `$GP*`; both `RMC` and
  `GGA` carry a position and either will do.
- Time on screen still comes from `SystemTime` (epoch at boot). The fix carries true UTC —
  wiring that through to the clock is a natural next step, not done here.
