# Spike 1 — hello on screen

Drives the M5StickC PLUS2's ST7789V2 panel with `mipidsi` + `embedded-graphics`: prints
`hello` and a tick counter that repaints once a second. Builds on spike 0's toolchain and
G4 power hold.

## Running

```sh
just flash     # build, flash, and tail the serial console
just monitor   # tail the console of whatever is already flashed
```

## What it established

The panel's pins, its 52,40 addressing offset, and why its SPI clock tops out at 26 MHz
rather than the 40 MHz M5GFX quotes are in [`docs/device.md`](../../../docs/device.md). All
three came from M5's own board definition rather than from guessing, and the offset is the
one that has to be right: without it the image shifts and wraps.

## Notes

- The backlight is only driven high after the first draw, so the panel doesn't briefly
  show whatever the controller powered up with.
- `mipidsi`'s `SpiInterface` is 4-line serial (a real DC pin). M5GFX's `spi_3wire = true`
  refers to its read path sharing MOSI, and doesn't apply here — nothing reads the panel.
