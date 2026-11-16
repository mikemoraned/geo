import { geoMercator } from "/vendor/d3-geo-3.1.1.js";
import { decodeTile } from "/vendor/maplibre-mlt-1.3.0.js";

const toUnitSquare = geoMercator().scale(1 / (2 * Math.PI)).translate([0.5, 0.5]);

export async function layerFeatures(archive, layer, zoom, bounds) {
  const tiles = await Promise.all(
    tilesCovering(zoom, bounds).map(({ x, y }) => archive.getZxy(zoom, x, y)),
  );
  return tiles
    .filter((tile) => tile !== undefined)
    .flatMap((tile) => decodeTile(new Uint8Array(tile.data)))
    .filter((table) => table.name === layer)
    .flatMap((table) => table.getFeatures().map((feature) => feature.properties));
}

export function tilesCovering(zoom, [west, south, east, north]) {
  const across = 2 ** zoom;
  const tileOf = (fraction) => Math.min(Math.floor(fraction * across), across - 1);
  const [left, top] = toUnitSquare([west, north]);
  const [right, bottom] = toUnitSquare([east, south]);
  const tiles = [];
  for (let x = tileOf(left); x <= tileOf(right); x++) {
    for (let y = tileOf(top); y <= tileOf(bottom); y++) {
      tiles.push({ x, y });
    }
  }
  return tiles;
}
