const MAPLIBRE = "/vendor/maplibre-gl-6.18.0.js";
const MAPLIBRE_WORKER = "/vendor/maplibre-gl-worker-6.18.0.js";
const MAPLIBRE_STYLESHEET = "/vendor/maplibre-gl-6.18.0.css";
const PMTILES = "/vendor/pmtiles-4.5.0.js";
const ROUTE_COLOUR = "#c33";

let loading;

function loadMapLibre() {
  loading ??= Promise.all([import(MAPLIBRE), import(PMTILES)]).then(([maplibre, { Protocol }]) => {
    const stylesheet = document.createElement("link");
    stylesheet.rel = "stylesheet";
    stylesheet.href = MAPLIBRE_STYLESHEET;
    document.head.append(stylesheet);
    maplibre.setWorkerUrl(new URL(MAPLIBRE_WORKER, location.href).href);
    maplibre.addProtocol("pmtiles", new Protocol().tile);
    return maplibre;
  });
  return loading;
}

export async function openMap(container, tiles) {
  const maplibre = await loadMapLibre();
  const map = new maplibre.Map({
    container,
    style: style(new URL(tiles, location.href).href),
    interactive: false,
  });
  await map.once("load");
  return {
    showJourney(journey, padding) {
      map.setFilter("route", ["==", ["get", "session"], journey.session]);
      const [west, south, east, north] = journey.bounds;
      map.fitBounds([[west, south], [east, north]], { padding, animate: false });
    },
    showSample(sample) {
      map.getSource("here").setData({
        type: "Point",
        coordinates: [sample.gps.lon, sample.gps.lat],
      });
    },
    remove() {
      map.remove();
    },
  };
}

function style(tiles) {
  return {
    version: 8,
    sources: {
      journeys: { type: "vector", url: `pmtiles://${tiles}`, encoding: "mlt" },
      here: { type: "geojson", data: { type: "FeatureCollection", features: [] } },
    },
    layers: [
      { id: "background", type: "background", paint: { "background-color": "#eef0ea" } },
      {
        id: "route",
        type: "line",
        source: "journeys",
        "source-layer": "route",
        filter: ["==", ["get", "session"], ""],
        layout: { "line-join": "round", "line-cap": "round" },
        paint: { "line-color": ROUTE_COLOUR, "line-width": 3 },
      },
      {
        id: "here",
        type: "circle",
        source: "here",
        paint: {
          "circle-radius": 7,
          "circle-color": ROUTE_COLOUR,
          "circle-stroke-color": "#fff",
          "circle-stroke-width": 2,
        },
      },
    ],
  };
}
