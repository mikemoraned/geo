# lookout_medallion

Writing the medallion store from python, so a derivation prototyped as a notebook produces
the same silver a Rust one does — see [`docs/medallion.md`](../../docs/medallion.md).

```python
import lookout_medallion

written = lookout_medallion.write_silver("train_segment", table)
written = lookout_medallion.write_silver("train_segment", table, root="/some/store")
```

`table` is anything exposing the Arrow PyCapsule interface: a pyarrow `Table`, a DuckDB
relation, a GeoDataFrame's `to_arrow()`. The rows are handed over as arrow rather than copied
through python objects.

The call **replaces the whole dataset**, since that is what a silver derivation does: a
partition the table covers is rewritten, and one it does not is deleted. So the table has to
hold every row of the dataset, not the rows that have changed.

The table's columns must be exactly the dataset's own, plus `geometry` and
`geometry_projected` where it carries geometry, plus the columns its partition values are
read from (`country`, and its date key where it has one). Those last are written into the
path rather than into the file. Geometry is WKB, or any GeoArrow encoding; the coordinates
are taken to be in the CRS the store states for each column — lat/lon for `geometry`, the
country's projected zone for `geometry_projected`.

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

## Building and testing

`uv` builds it with maturin, so nothing needs installing first:

```
just test-python        # from apps/lookout
```

That forces the extension to be rebuilt, since uv caches the wheel against this crate's own
sources and would otherwise not notice a change to the rust crates it wraps.
