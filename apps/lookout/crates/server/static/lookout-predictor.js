// The shell around the core: it loads the wasm, runs the clock, answers what the core asks
// for, and paints what it says. No prediction, no clock discipline and no formatting live
// here — the core holds all three, and answers an event that moved nothing with no request.
import init, { process_event, view } from "/wasm/lookout.js";

// Hoisted to module scope, so several elements on a page share one load. `connectedCallback`
// cannot be awaited by the browser, so the first paint waits on this promise instead.
const loaded = init();

const CROSSINGS = "/crossings.json";
const TICK_INTERVAL_MS = 1000;
const GEOLOCATION = { enableHighAccuracy: true, maximumAge: 0, timeout: 30_000 };

class LookoutPredictor extends HTMLElement {
  #timer;
  #watch;
  #screen;

  connectedCallback() {
    const root = this.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>
        :host { display: block; }
        .screen { font: 1rem ui-monospace, monospace; padding: 1rem; background: #032; color: #7f8; }
        .screen p { margin: 0 0 .3rem; }
      </style>
      <div class="screen">…</div>`;
    this.#screen = root.querySelector(".screen");

    loaded.then(() => {
      this.dispatch("Start");
      this.#timer = setInterval(() => this.#tick(), TICK_INTERVAL_MS);
      this.#watch = this.#follow();
    });
  }

  disconnectedCallback() {
    clearInterval(this.#timer);
    if (this.#watch !== undefined) navigator.geolocation.clearWatch(this.#watch);
  }

  // Every fix the browser will give, until the element goes away. Returns the watch id, or
  // undefined where there is no geolocation to read — which is also what a page served over
  // plain HTTP sees, since browsers withhold it outside a secure context.
  #follow() {
    if (!navigator.geolocation) {
      this.#screen.textContent = "no geolocation in this browser";
      return undefined;
    }
    return navigator.geolocation.watchPosition(
      (position) => this.#reported(position),
      (failed) => {
        this.#screen.textContent = `no position: ${failed.message}`;
      },
      GEOLOCATION,
    );
  }

  // One fix, as the browser measured it. Its own timestamp is sent, not the time it arrived:
  // it says where we were when it was taken, and an arrival counted from it is counted from
  // the right instant.
  #reported(position) {
    const { latitude, longitude, altitude, accuracy, speed, heading } = position.coords;
    this.dispatch({
      Position: {
        t: new Date(position.timestamp).toISOString(),
        gps: {
          lat: latitude,
          lon: longitude,
          alt: altitude,
          acc: accuracy,
          speed,
          heading,
        },
      },
    });
  }

  // The core refuses a time it has already counted, so sending one per interval is safe
  // however the browser schedules them.
  #tick() {
    this.dispatch({ Tick: new Date().toISOString() });
  }

  // Applies one event, doing what the core asks for and repainting only where it asked.
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
    const { now, crossings, here, predicted } = JSON.parse(view());
    this.#screen.innerHTML = `
      <p>${crossings.toLocaleString()} crossings</p>
      <p>${now ? new Date(now).toLocaleTimeString() : "no time yet"}</p>
      <p>${here ? `${here.position.y.toFixed(5)}, ${here.position.x.toFixed(5)}` : "no position yet"}</p>
      <p>${predicted.length} within range</p>`;
  }
}

customElements.define("lookout-predictor", LookoutPredictor);
