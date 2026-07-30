# Spike 1 — hello on screen

Drives the M5StickC PLUS2's ST7789V2 panel with `mipidsi` + `embedded-graphics`: prints
`hello` and a tick counter that repaints once a second. Builds on spike 0's toolchain and
G4 power hold.

## Running

```sh
just flash     # build, flash, and tail the serial console
just monitor   # tail the console of whatever is already flashed
```

## Panel configuration

Taken from M5's own board definition in
[M5GFX](https://github.com/m5stack/M5GFX/blob/master/src/M5GFX.cpp) (`board_M5StickCPlus2`),
not guessed:

| | |
|---|---|
| SCLK / MOSI | G13 / G15 |
| CS / DC / RST | G5 / G14 / G12 |
| Backlight | G27 |
| SPI host | SPI2 (HSPI), 26MHz — see below |
| Size | 135x240, **offset 52,40** |
| Colours | inverted |

The offset is the part that has to be right: the ST7789V2 addresses a window larger than
the visible 135x240 panel, so without it the image shifts and wraps.

Note the panel pins differ from the original StickC *Plus* (which used DC 23 / RST 18) —
Plus-era example code will not work here.

## M5GFX's 40MHz is not reachable

The panel is not on SPI2's IOMUX pins (CLK 14 / MOSI 13 / CS 15 — the board wires 13/15/5),
so the signals go through the GPIO matrix, which ESP-IDF caps at 80MHz/3 ≈ 26.67MHz. Asking
for 40MHz is rejected at `spi_bus_add_device`:

```
E spi_hal: The clock_speed_hz should less than 26666666
E spi_master: spi_bus_add_device(517): assigned clock speed not supported
```

which then boot-loops on a second assert as `SpiDriver::drop` tears down a bus that still
has the half-added device attached — the clock error above it is the real cause.

## Notes

- The backlight is only driven high after the first draw, so the panel doesn't briefly
  show whatever the controller powered up with.
- `mipidsi`'s `SpiInterface` is 4-line serial (a real DC pin). M5GFX's `spi_3wire = true`
  refers to its read path sharing MOSI, and doesn't apply here — nothing reads the panel.
