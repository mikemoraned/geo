import { geoMercator } from "/vendor/d3-geo-3.1.1.js";
import { decodeTile } from "/vendor/maplibre-mlt-1.3.0.js";
import { PMTiles } from "/vendor/pmtiles-4.5.0.js";

const toUnitSquare = geoMercator().scale(1 / (2 * Math.PI)).translate([0.5, 0.5]);

export async function readJourneys(url) {
  const archive = new PMTiles(url);
  const header = await archive.getHeader();
  const everywhere = [header.minLon, header.minLat, header.maxLon, header.maxLat];
  const routes = uniqueBy(
    await layerFeatures(archive, "route", header.minZoom, everywhere),
    (route) => route.session,
  );
  return Promise.all(routes.map((route) => readJourney(archive, route)));
}

async function readJourney(archive, route) {
  const bounds = [route.min_lon, route.min_lat, route.max_lon, route.max_lat];
  const inSession = (await layerFeatures(archive, "samples", route.samples_zoom, bounds)).filter(
    (sample) => sample.session === route.session,
  );
  const samples = uniqueBy(inSession, (sample) => sample.seq).sort((a, b) => a.seq - b.seq);
  return {
    session: route.session,
    crossings: route.crossings,
    bounds,
    samples: samples.map(asSample),
  };
}

function asSample(properties) {
  return {
    t: properties.t,
    gps: {
      lat: properties.lat,
      lon: properties.lon,
      alt: properties.alt ?? null,
      acc: properties.acc ?? null,
      speed: properties.speed ?? null,
      heading: properties.heading ?? null,
    },
  };
}

async function layerFeatures(archive, layer, zoom, bounds) {
  const tiles = await Promise.all(
    tilesCovering(zoom, bounds).map(({ x, y }) => archive.getZxy(zoom, x, y)),
  );
  return tiles
    .filter((tile) => tile !== undefined)
    .flatMap((tile) => decodeTile(new Uint8Array(tile.data)))
    .filter((table) => table.name === layer)
    .flatMap((table) => table.getFeatures().map((feature) => feature.properties));
}

function tilesCovering(zoom, [west, south, east, north]) {
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

function uniqueBy(items, key) {
  return [...new Map(items.map((item) => [key(item), item])).values()];
}
