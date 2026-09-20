# The browser

The same core the board runs, compiled to WebAssembly by `wasm-pack`. The board's
counterpart is [device.md](device.md).

Hand-written pages, no framework and no build step, so the core is reached over JSON rather
than a generated binding.

A Web Component owns the core and draws it, repainting when the core asks. A page adds it by
writing a tag and sends it positions and time; where those come from is all that separates
one page from another.
