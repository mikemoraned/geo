# lookout_medallion

The medallion store from python. Why the way in is a binding rather than a second implementation,
and what a table has to carry, is
[writing silver from another language](../../docs/medallion.md#writing-silver-from-another-language).
What follows is only what a python caller has to know.

Each function's own documentation is its docstring, which `pyo3` publishes from the doc comment in
`src/lib.rs`, so `help(lookout_medallion.write_silver)` in a notebook is the reference.

```python
import lookout_medallion

written = lookout_medallion.write_silver("train_segment", table)
written = lookout_medallion.write_silver("train_segment", table, root="/some/store")

table = lookout_medallion.query_silver(
    "SELECT trip_id, ST_X(geometry) AS lon FROM train_segment WHERE country = $country",
    params={"country": "DE"},
)
```

## Handing a table over

A table crossing either way is anything exposing the Arrow PyCapsule interface — a pyarrow table, a
DuckDB result, a GeoDataFrame's `to_arrow()` — so nothing is copied through python objects. Geometry
goes in as WKB or as any GeoArrow encoding, whichever the library at hand produces.

Projecting is the caller's work: a notebook projects the coordinates itself and asks
`projected_crs(country)` for the zone, rather than naming one of its own.

Both calls release the interpreter while they run, since the work is filesystem work that calls back
into nothing python owns.

## What is raised

A mistake in the call raises `ValueError` naming what was wrong — an unknown dataset, one outside
silver, a missing or unexpected column, a column that cannot be read as the type the dataset
defines, a country the store does not know. A failure while reading or writing the files raises
`RuntimeError`, so a notebook can tell a typo from a broken store.

## Using it from a notebook

marimo notebooks here run `--sandbox`, so the dependency goes in the notebook's own inline script
metadata, with the path resolved relative to the notebook:

```python
# /// script
# dependencies = ["lookout-medallion", ...]
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///
```

Run such a notebook with `uv run --no-project --reinstall-package lookout-medallion
<notebook>.py`: uv caches the built wheel against this crate's own sources, and would otherwise not
notice a change to the rust crates it wraps. `just test-python` (from `apps/lookout`) runs the tests
the same way; nothing needs installing first, since uv builds the extension with maturin.
