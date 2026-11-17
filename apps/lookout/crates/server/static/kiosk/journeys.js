import { PMTiles } from "/vendor/pmtiles-4.5.0.js";
import { layerFeatures } from "/tiles.js";

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

function uniqueBy(items, key) {
  return [...new Map(items.map((item) => [key(item), item])).values()];
}
