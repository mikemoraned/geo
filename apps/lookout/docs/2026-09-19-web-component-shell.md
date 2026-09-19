# Embedding the core in a web component

*Note of 2026-09-19, written before any web shell existed. It records the shape a
browser shell would take against the core as it stands on crux 0.16.2, not a shell that was
built. What is durable is the shape — three events in, one panel out, a single `Render`
effect — and it holds whatever the shell is written in. What goes stale is the plumbing:
the generated `shared_types` import paths and the TypeScript names for `DateTime` and
`Sentence`, which move with the typegen build and the crux version.*

The `Lookout` core knows nothing about what renders it. Compiled to WebAssembly with
`wasm-pack`, it is three functions over Bincode-serialized bytes: `process_event`, `view`,
and `handle_response`. A Web Component is a natural shell for it because the core is pure —
all NMEA parsing, prediction, battery interpretation and clock discipline live inside — so
the shell is left with nothing but I/O.

## The shell's job

Produce three events, and paint one panel. Nothing else. A custom element extends
`HTMLElement`, owns the wasm instance, and holds the screen. Its lifecycle callbacks map
onto the work: `connectedCallback` loads the wasm and starts the clock, `disconnectedCallback`
stops it.

## The one effect is `Render`

`Effect` has a single variant, and `update` returns either `render::render()` or
`Command::done()`. So there is no `handle_response` loop: every `process_event` comes back
as one `Render` request or as nothing at all. The shell repaints on `Render` and does
nothing on empty.

The empty case is not an edge to smooth over. A dozen sentences a second arrive and most
change nothing, and the core answers those with no request precisely so the screen is not
redrawn a dozen times a second. Do not repaint on every dispatch; honour the empty response.

## Inbound events

Each maps to a browser source:

- **`Tick(DateTime<Utc>)`** — a one-second `setInterval` sending the current time. The core
  refuses a tick behind the receiver's own clock, so a naive loop is safe; it need not be
  clever.
- **`Sentence(Sentence)`** — one line off the wire. In a browser that is the Web Serial API
  (`navigator.serial`) for a real receiver, or a WebSocket or replay stream for a simulator.
- **`Battery(u16)`** — terminal voltage in millivolts, from wherever it is measured.

## The panel

`ViewModel` is already a fixed-width character panel: `clock`, `latitude`, `longitude`,
`battery`, `quality`, `within`, and `nearest` (a list of lines). Rendered into a shadow root
as a monospace screen, it needs no reshaping.

## Sketch

```js
import init, { process_event, view } from "lookout_core"; // your wasm-pack package
import {
  ViewModel, Request, EffectVariantRender,
  EventVariantTick, EventVariantSentence, EventVariantBattery,
} from "shared_types/types/shared_types";
import { BincodeSerializer, BincodeDeserializer } from "shared_types/bincode/mod";

function requests(bytes) {
  const d = new BincodeDeserializer(bytes);
  const n = d.deserializeLen();
  const out = [];
  for (let i = 0; i < n; i++) out.push(Request.deserialize(d));
  return out;
}

class LookoutPanel extends HTMLElement {
  #timer;

  connectedCallback() {
    this.attachShadow({ mode: "open" });
    init().then(() => {
      this.#paint();
      this.#timer = setInterval(
        () => this.dispatch(new EventVariantTick(/* DateTime<Utc> — see Gotchas */)),
        1000,
      );
    });
  }

  disconnectedCallback() {
    clearInterval(this.#timer);
  }

  dispatch(event) {
    const s = new BincodeSerializer();
    event.serialize(s);
    const reqs = requests(process_event(s.getBytes()));
    if (reqs.some((r) => r.effect instanceof EffectVariantRender)) this.#paint();
  }

  #paint() {
    const vm = ViewModel.deserialize(new BincodeDeserializer(view()));
    this.shadowRoot.innerHTML = `
      <style>pre { font: 16px/1.3 monospace; background: #032; color: #7f8; padding: .5rem; }</style>
      <pre>${vm.clock}
${vm.latitude} ${vm.longitude}
${vm.quality}   ${vm.battery}
${vm.within}
${vm.nearest.join("\n")}</pre>`;
  }
}

customElements.define("lookout-panel", LookoutPanel);
```

A Web Serial reader then feeds lines in with `this.dispatch(new EventVariantSentence(...))`,
one per line. Swapping `HTMLElement` for `LitElement` leaves the dispatch logic untouched;
only `#paint` changes.

## Rust side

The `shared` crate exposes the bridge to `wasm-bindgen`:

```rust
lazy_static! {
    static ref CORE: Bridge<Effect, App> = Bridge::new(Core::new());
}

#[wasm_bindgen]
pub fn process_event(data: &[u8]) -> Vec<u8> { CORE.process_event(data) }

#[wasm_bindgen]
pub fn view() -> Vec<u8> { CORE.view() }
```

`handle_response` is wired the same way but is unused while `Render` is the only effect.
Check the exact `Bridge<…>` type parameters and whether `Core::new()` takes a turbofish
against `examples/counter` at the 0.16.2 tag — those are the two spots that moved around this
version.

## Gotchas

- **`DateTime<Utc>` and `Sentence` are not primitives.** Both must be registered in typegen
  so TypeScript bindings exist, and constructing `EventVariantTick` and `EventVariantSentence`
  means building whatever `serde-generate` emitted for them. Check what `DateTime<Utc>`
  renders to — a small `toGeneratedDateTime(new Date())` helper keeps the tick loop readable
  — and whether `Sentence` is a thin string newtype or carries more.
- **Async init, sync lifecycle.** `connectedCallback` cannot be awaited by the browser, so
  gate the first paint on the init promise.
- **Many instances.** Hoist `init()` to module scope so the wasm module loads once rather
  than per element.

## References

- [crux_core 0.16.2 README](https://docs.rs/crate/crux_core/0.16.2/source/README.md) — the
  update signature, the FFI, and the Command-over-Capabilities note for this version
- [examples/counter](https://github.com/redbadger/crux/tree/master/examples/counter) —
  rewritten for the Command API; the reference for the FFI boilerplate at this tag
- [Lit — defining a component](https://lit.dev/docs/components/defining/) — if the panel
  moves to `LitElement`
