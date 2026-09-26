# web

The browser as a shell: `core` is what a browser makes of
[`platform-core`](../../platform-core/README.md), and `bridge` is that core as two functions over
JSON. What a page does with them is [the browser](../../../docs/web.md).

What only a browser has is here — a set arriving over the network rather than sitting in flash, and
positions rather than pixels for whatever draws them.

## The set is read once, while projecting

A prediction names a crossing by its compact id, and the set it was predicted against is the only
place that id means anything. So the view reads the set once as it projects, rather than once per
prediction, and drops a crossing the set no longer holds: the canvas draws a position, and there is
none for it.

## The tests are the JSON contract

Nothing checks the shapes across the bridge, so the tests do. They assert the exact JSON of an
event, a request and a view, so a change to any of the three fails here rather than in a browser.

Each exported function wraps one answering a plain `Result<String, String>`, since a `JsError` can
only be built on wasm. One core is shared by the whole process, so a page's sequence is one ordered
test rather than several sharing state.
