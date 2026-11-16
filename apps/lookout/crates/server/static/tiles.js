import { geoMercator } from "/vendor/d3-geo-3.1.1.js";
import { WKBWriter } from "/vendor/loaders-gl-wkt-4.5.4.js";
import { GEOMETRY_TYPE, decodeTile } from "/vendor/maplibre-mlt-1.3.0.js";

const toUnitSquare = geoMercator().scale(1 / (2 * Math.PI)).translate([0.5, 0.5]);

export async function layerFeatures(archive, layer, zoom, bounds) {
  return (await layerTables(archive, layer, zoom, bounds)).flatMap(({ table }) =>
    table.getFeatures().map((feature) => feature.properties),
  );
}

export async function layerLines(archive, layer, zoom, bounds) {
  return (await layerTables(archive, layer, zoom, bounds)).flatMap(({ x, y, table }) => {
    const toLonLat = ({ x: across, y: down }) =>
      toUnitSquare.invert([
        (x + across / table.extent) / 2 ** zoom,
        (y + down / table.extent) / 2 ** zoom,
      ]);
    return table
      .getFeatures()
      .map(({ geometry }) => asLonLat(geometry, toLonLat))
      .filter((geometry) => geometry !== undefined)
      .map((geometry) => new Uint8Array(WKBWriter.encodeSync(geometry)));
  });
}

function asLonLat({ type, coordinates }, toLonLat) {
  const lines = coordinates.map((line) => line.map(toLonLat));
  switch (type) {
    case GEOMETRY_TYPE.LINESTRING:
      return { type: "LineString", coordinates: lines[0] };
    case GEOMETRY_TYPE.MULTILINESTRING:
      return { type: "MultiLineString", coordinates: lines };
    default:
      return undefined;
  }
}

async function layerTables(archive, layer, zoom, bounds) {
  const tiles = await Promise.all(
    tilesCovering(zoom, bounds).map(async ({ x, y }) => ({
      x,
      y,
      tile: await archive.getZxy(zoom, x, y),
    })),
  );
  return tiles
    .filter(({ tile }) => tile !== undefined)
    .flatMap(({ x, y, tile }) =>
      decodeTile(new Uint8Array(tile.data))
        .filter((table) => table.name === layer)
        .map((table) => ({ x, y, table })),
    );
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
