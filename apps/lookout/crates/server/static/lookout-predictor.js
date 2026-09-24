import init, { process_event, view } from "/wasm/lookout.js";
import { geoAzimuthalEquidistant } from "/vendor/d3-geo-3.1.1.js";
import { scaleSymlog } from "/vendor/d3-scale-4.0.2.js";

const loaded = init();

const CROSSINGS = "/crossings.json";
const LINEAR_WITHIN_METRES = 500;
const CANVAS_SIZE = 320;
const TAU = Math.PI * 2;

function speed(metres_per_second) {
  if (metres_per_second === null || metres_per_second === undefined) return "no speed yet";
  return `${(metres_per_second * 3.6).toFixed(0)} km/h`;
}

class LookoutPredictor extends HTMLElement {
  #screen;
  #canvas;

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

  #paint() {
    const { now, crossings, radius_metres, here, predicted } = JSON.parse(view());
    this.#draw(here, predicted, radius_metres);
    this.#screen.textContent = [
      now ? new Date(now).toLocaleTimeString() : "no time yet",
      here ? speed(here.speed_mps) : "no position yet",
      here
        ? `${predicted.length} within ${(radius_metres / 1000).toFixed(0)}km`
        : `${crossings.toLocaleString()} crossings`,
    ].join("\n");
  }

  #draw(here, predicted, maximum) {
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

    context.fillStyle = "#7f8";
    for (const crossing of predicted) {
      const [x, y] = project([crossing.position.x, crossing.position.y]);
      const angle = Math.atan2(x, -y);
      const out = reach(crossing.metres);
      context.beginPath();
      context.arc(middle + out * Math.sin(angle), middle - out * Math.cos(angle), 3, 0, TAU);
      context.fill();
    }

    context.fillStyle = "#cfd";
    context.beginPath();
    context.arc(middle, middle, 3 + Math.min(here.speed_mps ?? 0, 30) / 3, 0, TAU);
    context.fill();
  }
}

customElements.define("lookout-predictor", LookoutPredictor);
