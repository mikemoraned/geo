# lookout

Tells you what is coming up as you travel: which water crossings you are about to pass, how far
off they are, and when you reach them at the speed you are going. It runs on [an M5StickC
PLUS2](docs/device.md) and [in a browser](docs/web.md), over the same core.

What is here, and where to read about it:

- [architecture.md](docs/architecture.md) — how an observation reaches the store, and what is
  derived from it.
- [medallion.md](docs/medallion.md) — the store the pipeline fills.
- [telemetry.md](docs/telemetry.md), [overture.md](docs/overture.md),
  [motis.md](docs/motis.md) — the wire a device sends, and the two upstream sources.
- [device.md](docs/device.md), [web.md](docs/web.md) — what each platform is like.
- `crates/*/README.md` — what each crate decides.

## Running the recipes

`just` recipes run from this directory, and `just --list` is the index. Setting up a machine is
`just prerequisites`: `rerun` and `wasm-pack` from cargo, then DuckDB for the multi-engine test,
PROJ and jq for `crs-definitions`, and the wasm target.

A fresh worktree carries bronze and derives the rest: `just init` fills in the Overture extract
and re-derives every silver dataset. The extract comes from a local mirror of the Overture bucket
when its drive is mounted, and from the public bucket otherwise; no live source is touched either
way.

Gold is packed when it is needed rather than by `init`, and each packing adopts what it wrote:
`crossings.version` and `sessions.version` name the versions in play. The device's build script
reads the first to decide what to embed, and `just deploy` passes both to the image build, so a
board and a page are built against the same crossings.

## What is generated

The wasm module, and the crossings and sessions a page fetches, are copied into the directory
`server` serves them from. All three are gitignored: the Docker builder stage runs the same
commands for a deploy, so what is served is built rather than committed.

## Testing

`just test` runs everything, `just test-no-docker` everything a sandbox can. Both run the doctests
separately from nextest, which does not support them, and both rebuild the two python extensions —
uv's wheel cache does not notice a change to the rust crates underneath.
