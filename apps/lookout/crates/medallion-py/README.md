# lookout_medallion

Writing the medallion store from python, so a derivation prototyped as a notebook produces
the same silver a Rust one does — see [`docs/medallion.md`](../../docs/medallion.md) for the
store itself, and `src/lib.rs` for the API, whose doc comments are the module's `__doc__`.

```python
import lookout_medallion

written = lookout_medallion.write_silver("train_segment", table)
written = lookout_medallion.write_silver("train_segment", table, root="/some/store")
```

## What the table has to hold

The call **replaces the whole dataset**, so the table has to hold every row of it, not the
rows that have changed: a partition the table covers is rewritten, and one it does not is
deleted.

Its columns must be exactly the dataset's own, plus `geometry` and `geometry_projected`
where it carries geometry, plus the columns its partition values are read from (`country`,
and its date key where it has one). Those last are written into the path rather than into
the file. Geometry is WKB, or any GeoArrow encoding; the coordinates are taken to be in the
CRS the store states for each column — lat/lon for `geometry`, the country's projected zone
for `geometry_projected`.

Anything else is refused with a `ValueError` naming what was wrong: an unknown dataset, one
outside silver, a missing or unexpected column, a column that cannot be read as the type the
dataset defines, or a country the store does not know.

## Using it from a notebook

marimo notebooks here run `--sandbox`, so the dependency goes in the notebook's own inline
script metadata, with the path resolved relative to the notebook:

```python
# /// script
# dependencies = ["lookout-medallion", ...]
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///
```

Run such a notebook with `uv run --no-project --reinstall-package lookout-medallion
<notebook>.py`: uv caches the built wheel against this crate's own sources, and would
otherwise not notice a change to the rust crates it wraps. `just test-python` (from
`apps/lookout`) runs the tests the same way; nothing needs installing first, since uv builds
the extension with maturin.
