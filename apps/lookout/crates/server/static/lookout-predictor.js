// The core, in an element: it loads the wasm, answers what the core asks for, and paints what
// it says. No prediction, no clock discipline and no formatting live here — the core holds all
// three, and answers an event that moved nothing with no request.
//
// Where positions and time come from is not its business either. A page sends them through
// `dispatch`, which is what lets one element serve a live receiver and a replay.
import init, { process_event, view } from "/wasm/lookout.js";
import { geoAzimuthalEquidistant } from "/vendor/d3-geo-3.1.1.js";
import { scaleSymlog } from "/vendor/d3-scale-4.0.2.js";

// Hoisted to module scope, so several elements on a page share one load. `connectedCallback`
// cannot be awaited by the browser, so the first paint waits on this promise instead.
const loaded = init();

const CROSSINGS = "/crossings.json";
// How far out the scale stays close to linear, in metres. Below it a crossing moves across
// the picture about as fast as it moves over the ground; above it, distances compress. So the
// smaller this is, the more of the picture goes to what is close — which is what is about to
// matter.
const LINEAR_WITHIN_METRES = 500;
const CANVAS_SIZE = 320;
const TAU = Math.PI * 2;

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
        .status { margin: .6rem 0 0; }
      </style>
      <div class="screen">
        <canvas></canvas>
        <p class="status">…</p>
      </div>`;
    this.#screen = root.querySelector(".status");
    this.#canvas = root.querySelector("canvas");
    // Drawn at the device's own resolution, so the dots are not blurred on a phone.
    const density = window.devicePixelRatio || 1;
    this.#canvas.width = CANVAS_SIZE * density;
    this.#canvas.height = CANVAS_SIZE * density;
    this.#canvas.getContext("2d").scale(density, density);

    // What it is fed, and when it is started, are the page's business.
    this.ready = loaded;
  }

  // Applies one event, doing what the core asks for and repainting only where it asked. This
  // is the whole surface a page drives: a position, or a time.
  async dispatch(event) {
    const requests = JSON.parse(process_event(JSON.stringify(event)));
    let moved = false;
    for (const request of requests) {
      if ("Render" in request.effect) moved = true;
      if ("Crossings" in request.effect) await this.#fetchCrossings();
    }
    if (moved) this.#paint();
  }

  // The core asks once, when it has no set to predict against. A failure leaves it asking for
  // nothing more, so the page says so rather than sitting blank.
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
    const { crossings, radius_metres, here, predicted } = JSON.parse(view());
    this.#draw(here, predicted, radius_metres);
    this.#screen.textContent = here
      ? `${predicted.length} within ${(radius_metres / 1000).toFixed(0)}km of ${crossings.toLocaleString()}`
      : `${crossings.toLocaleString()} crossings, waiting for a position`;
  }

  // Where we are in the middle, what is about to be crossed around it, and nothing beyond the
  // radius: a dot on the rim is a crossing at the furthest the core looks.
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

    // Centred on us and true in every direction from there, so a crossing is drawn where it
    // actually lies; only how far out is remapped.
    const project = geoAzimuthalEquidistant()
      .rotate([-here.position.x, -here.position.y])
      .translate([0, 0])
      .scale(1);

    // Near distances given more of the picture than far ones, and the furthest the core looks
    // landing on the rim. Symmetric-log rather than log because a crossing can be underneath
    // us, and log has nowhere to put nought.
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

    // Us, sized by how fast the core reckons we are going.
    context.fillStyle = "#cfd";
    context.beginPath();
    context.arc(middle, middle, 3 + Math.min(here.speed_mps ?? 0, 30) / 3, 0, TAU);
    context.fill();
  }
}

customElements.define("lookout-predictor", LookoutPredictor);
