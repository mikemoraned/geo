# /// script
# dependencies = [
#     "lookout-medallion",
#     "marimo",
#     "pmtiles==3.8.1",
#     "polars==2.1.0",
# ]
# requires-python = ">=3.13"
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import polars as pl
    from pathlib import Path
    from pmtiles.reader import MmapSource, Reader, all_tiles

    import lookout_medallion

    return MmapSource, Path, Reader, all_tiles, lookout_medallion, mo, pl


@app.cell
def _(mo):
    mo.md(r"""
    # The map's tiles

    The kiosk reads `map.pmtiles` through range requests, one tile at a time, so what a client
    fetches is a tile rather than the file. This notebook reads the newest packed map in gold, and
    measures its tiles at each zoom level.
    """)
    return


@app.cell
def _(MmapSource, Path, Reader, lookout_medallion, mo):
    MAP = (
        max(
            (
                Path(lookout_medallion.default_root())
                / "gold"
                / "artifact=map"
            ).iterdir()
        )
        / "map.pmtiles"
    )

    with MAP.open("rb") as _file:
        header = Reader(MmapSource(_file)).header()

    mo.vstack(
        [
            mo.md(f"`{MAP.parent.name}`, {MAP.stat().st_size / 1e6:.1f} MB"),
            header,
        ]
    )
    return (MAP,)


@app.cell
def _(MAP, MmapSource, all_tiles, pl):
    with MAP.open("rb") as _file:
        tiles = pl.DataFrame(
            [
                {"z": z, "x": x, "y": y, "bytes": len(data)}
                for (z, x, y), data in all_tiles(MmapSource(_file))
            ]
        )

    tiles.height
    return (tiles,)


@app.cell
def _(mo):
    mo.md(r"""
    Each zoom level's tiles. Samples and routes alone reach zoom level 6, and the base map starts at 7.
    """)
    return


@app.cell
def _(pl, tiles):
    tiles.group_by("z").agg(
        tiles=pl.len(),
        total_mb=(pl.col("bytes").sum() / 1e6).round(2),
        median_kb=(pl.col("bytes").median() / 1e3).round(1),
        p95_kb=(pl.col("bytes").quantile(0.95) / 1e3).round(1),
        largest_kb=(pl.col("bytes").max() / 1e3).round(1),
    ).sort("z")
    return


@app.cell
def _(mo):
    mo.md(r"""
    The largest tiles in the file.
    """)
    return


@app.cell
def _(pl, tiles):
    tiles.sort("bytes", descending=True).head(10).with_columns(
        kb=(pl.col("bytes") / 1e3).round(1)
    )
    return


if __name__ == "__main__":
    app.run()
