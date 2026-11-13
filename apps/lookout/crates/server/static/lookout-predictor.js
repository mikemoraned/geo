import init, { process_event, view } from "/wasm/lookout.js";
import { geoAzimuthalEquidistant, geoDistance } from "/vendor/d3-geo-3.1.1.js";
import { scaleSymlog } from "/vendor/d3-scale-4.0.2.js";
import { WKBLoader } from "/vendor/loaders-gl-wkt-4.5.4.js";
import { PMTiles } from "/vendor/pmtiles-4.5.0.js";
import { layerLines } from "/tiles.js";

const loaded = init();

const CROSSINGS = "/crossings.json";
const MAP = "/map.pmtiles";
const RAIL_ZOOM = 12;
const LINEAR_WITHIN_METRES = 500;
const CANVAS_SIZE = 320;
const TAU = Math.PI * 2;
const EARTH_RADIUS_METRES = 6_371_008.8;
const LINE_STYLES = {
  rail: { colour: "#3a6", width: 1.5, dash: [3, 2] },
  trace: { colour: "rgba(204, 255, 221, 0.6)", width: 2, dash: [] },
};

let map;

function base64(bytes) {
  return btoa(Array.from(bytes, (byte) => String.fromCharCode(byte)).join(""));
}

function geometry(wkb) {
  const bytes = Uint8Array.from(atob(wkb), (character) => character.charCodeAt(0));
  return WKBLoader.parseSync(bytes.buffer);
}

function polylines({ type, coordinates }) {
  return type === "MultiLineString" ? coordinates : [coordinates];
}
const RING_DASHES = { alerted_pass: [2, 2], actual_pass: [] };

function speed(metres_per_second) {
  if (metres_per_second === null || metres_per_second === undefined) return "no speed yet";
  return `${(metres_per_second * 3.6).toFixed(0)} km/h`;
}

class LookoutPredictor extends HTMLElement {
  #screen;
  #canvas;
  #rail;

  connectedCallback() {
    const root = this.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>
        :host { display: block; font: 0.9rem ui-monospace, monospace; color: #7f8; }
        .screen { display: inline-block; padding: 1rem; background: #032; }
        canvas { display: block; width: ${CANVAS_SIZE}px; height: ${CANVAS_SIZE}px; }
        .status { margin: .6rem 0 0; white-space: pre-line; }
      </style>
      <div class="screen">
        <canvas></canvas>
        <p class="status">…</p>
      </div>`;
    this.#screen = root.querySelector(".status");
    this.#canvas = root.querySelector("canvas");
    const density = window.devicePixelRatio || 1;
    this.#canvas.width = CANVAS_SIZE * density;
    this.#canvas.height = CANVAS_SIZE * density;
    this.#canvas.getContext("2d").scale(density, density);

    this.ready = loaded;
  }

  async dispatch(event) {
    const requests = JSON.parse(process_event(JSON.stringify(event)));
    let moved = false;
    for (const request of requests) {
      if ("Render" in request.effect) moved = true;
      if ("Crossings" in request.effect) await this.#fetchCrossings();
      if ("Lines" in request.effect) this.#fetchRail(request.effect.Lines);
    }
    if (moved) this.#paint();
  }

  async #fetchCrossings() {
    try {
      const response = await fetch(CROSSINGS);
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      await this.dispatch({ Crossings: await response.json() });
    } catch (failed) {
      this.#screen.textContent = `could not load ${CROSSINGS}: ${failed.message}`;
    }
  }

  async #fetchRail({ id, area }) {
    this.#rail?.abort();
    const asked = new AbortController();
    this.#rail = asked;
    try {
      map ??= new PMTiles(MAP);
      const { xmin, ymin, xmax, ymax } = area;
      const bounds = [xmin, ymin, xmax, ymax];
      const rail = await layerLines(map, "rail", RAIL_ZOOM, bounds, asked.signal);
      await this.dispatch({ Lines: { id, lines: rail.map(base64) } });
    } catch (failed) {
      if (!asked.signal.aborted) {
        this.#screen.textContent = `could not load the rail from ${MAP}: ${failed.message}`;
      }
    }
  }

  #paint() {
    const { now, crossings, radius_metres, here, predicted, lines } = JSON.parse(view());
    this.#draw(here, predicted, lines, radius_metres);
    this.#screen.textContent = [
      now ? new Date(now).toLocaleTimeString() : "no time yet",
      here ? speed(here.speed_mps) : "no position yet",
      here
        ? `${predicted.length} within ${(radius_metres / 1000).toFixed(0)}km`
        : `${crossings.toLocaleString()} crossings`,
    ].join("\n");
  }

  #draw(here, predicted, lines, maximum) {
    const context = this.#canvas.getContext("2d");
    const middle = CANVAS_SIZE / 2;
    const radius = middle - 4;
    context.clearRect(0, 0, CANVAS_SIZE, CANVAS_SIZE);

    context.strokeStyle = "#175";
    for (const fraction of [1, 0.5]) {
      context.beginPath();
      context.arc(middle, middle, radius * fraction, 0, TAU);
      context.stroke();
    }

    if (!here) return;

    const project = geoAzimuthalEquidistant()
      .rotate([-here.position.x, -here.position.y])
      .translate([0, 0])
      .scale(1);

    const reach = scaleSymlog()
      .domain([0, maximum])
      .range([0, radius])
      .constant(LINEAR_WITHIN_METRES)
      .clamp(true);

    const centre = [here.position.x, here.position.y];
    const beyond = reach.copy().clamp(false);
    const place = (point) => {
      const [x, y] = project(point);
      const angle = Math.atan2(x, -y);
      const out = beyond(geoDistance(centre, point) * EARTH_RADIUS_METRES);
      return [middle + out * Math.sin(angle), middle - out * Math.cos(angle)];
    };

    context.save();
    context.beginPath();
    context.arc(middle, middle, radius, 0, TAU);
    context.clip();
    for (const { category, wkb } of lines) {
      const style = LINE_STYLES[category];
      context.strokeStyle = style.colour;
      context.lineWidth = style.width;
      context.setLineDash(style.dash);
      for (const polyline of polylines(geometry(wkb))) {
        context.beginPath();
        for (const point of polyline) context.lineTo(...place(point));
        context.stroke();
      }
    }
    context.restore();

    context.fillStyle = "#7f8";
    context.strokeStyle = "#7f8";
    for (const crossing of predicted) {
      const [x, y] = project([crossing.position.x, crossing.position.y]);
      const angle = Math.atan2(x, -y);
      const out = reach(crossing.metres);
      const across = middle + out * Math.sin(angle);
      const down = middle - out * Math.cos(angle);
      context.beginPath();
      context.arc(across, down, 3, 0, TAU);
      context.fill();
      const dashes = RING_DASHES[crossing.state];
      if (dashes) {
        context.setLineDash(dashes);
        context.beginPath();
        context.arc(across, down, 6, 0, TAU);
        context.stroke();
      }
    }
    context.setLineDash([]);

    context.fillStyle = "#cfd";
    context.beginPath();
    context.arc(middle, middle, 3 + Math.min(here.speed_mps ?? 0, 30) / 3, 0, TAU);
    context.fill();
  }
}

customElements.define("lookout-predictor", LookoutPredictor);
