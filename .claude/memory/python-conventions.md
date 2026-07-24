# Python conventions

How Python is written and placed in this monorepo.

- **Keep Python to a minimum.** Prefer a shell one-liner or an existing tool over a script.
  Only write Python for genuinely awkward logic (e.g. a keyed CSV join over quoted,
  comma-containing fields, where `awk` is fragile — see `tools/pfaedle/splice`).
- **Put it in a uv project directory**, with its own `pyproject.toml`, run via
  `uv run --project <dir> python <script>` — never a loose `.py` invoked with bare `python3`.
  Examples: `apps/lookout/visualise`, `questions/*`, `tools/pfaedle/splice`.
- **No low-level fiddliness.** Reach for stdlib helpers — `shutil.copyfileobj` (not manual
  `iter(lambda: f.read(1<<20), b"")` chunk loops), `csv.DictReader`/`DictWriter`, etc.
