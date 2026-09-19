// The shell around the core: it loads the wasm, runs the clock, and paints what the core
// says. No prediction, no clock discipline and no formatting live here — the core holds all
// three, and answers an event that moved nothing with no request at all.
import init, { process_event, view } from "/wasm/lookout.js";

// Hoisted to module scope, so several elements on a page share one load. `connectedCallback`
// cannot be awaited by the browser, so the first paint waits on this promise instead.
const loaded = init();

const TICK_INTERVAL_MS = 1000;

class LookoutPredictor extends HTMLElement {
  #timer;
  #screen;

  connectedCallback() {
    const root = this.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>
        :host { display: inline-block; }
        .screen { font: 2rem ui-monospace, monospace; padding: 1rem; background: #032; color: #7f8; }
      </style>
      <div class="screen">…</div>`;
    this.#screen = root.querySelector(".screen");

    loaded.then(() => {
      this.#paint();
      this.#timer = setInterval(() => this.#tick(), TICK_INTERVAL_MS);
    });
  }

  disconnectedCallback() {
    clearInterval(this.#timer);
  }

  // The core refuses a time it has already counted, so sending one per interval is safe
  // however the browser schedules them.
  #tick() {
    this.dispatch({ Tick: new Date().toISOString() });
  }

  // Applies one event, repainting only where the core asked for it.
  dispatch(event) {
    const requests = JSON.parse(process_event(JSON.stringify(event)));
    if (requests.some((request) => "Render" in request.effect)) this.#paint();
  }

  #paint() {
    this.#screen.textContent = JSON.parse(view()).count;
  }
}

customElements.define("lookout-predictor", LookoutPredictor);
