# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "scipy",
#     "geopandas",
#     "numpy",
#     "matplotlib",
#     "shapely",
#     "contextily",
#     "marimo",
# ]
# ///

import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    city_name = "edinburgh"
    return (city_name,)


@app.cell
def _():
    import geopandas as gpd
    import numpy as np
    import marimo as mo
    import matplotlib.pyplot as plt
    from scipy.interpolate import griddata
    from shapely import vectorized
    import contextily as cx
    from matplotlib.colors import BoundaryNorm
    return BoundaryNorm, gpd, mo, np, plt


@app.cell
def _():
    h3_resolution = 10
    epsg_metres = 27700  # a CRS that has unit of metres; use this here to avoid warnings about area calculations
    return epsg_metres, h3_resolution


@app.cell
def _(city_name, h3_resolution):
    arrow_data_file = f"{city_name}_min_{h3_resolution}.arrow"
    return (arrow_data_file,)


@app.cell
def _(arrow_data_file, epsg_metres, gpd):
    heights_gdf = gpd.read_feather(arrow_data_file).to_crs(epsg=epsg_metres).copy()
    return (heights_gdf,)


@app.cell
def _(BoundaryNorm, mo, np, plt):
    @mo.cache
    def make_norm(heights_gdf):
        min_height = heights_gdf[["min_height"]]
        percentiles = np.linspace(0, 100, 41)
        bounds = np.percentile(min_height, percentiles)

        norm = BoundaryNorm(bounds, ncolors=256)

        return norm

    def plot_boundary(gdf, ax):
        gdf.dissolve().boundary.plot(ax=ax, edgecolor="black", linewidth=0.5)


    @mo.cache
    def make_header_plot(heights_gdf, height_thresholds=[]):
        fig, ax = plt.subplots(figsize=(8, 8))
        ax.set_axis_off()

        plot_boundary(heights_gdf, ax)

        norm = make_norm(heights_gdf)

        heights_gdf.plot("min_height", ax=ax, cmap="terrain", norm=norm)

        ax_inset = ax.inset_axes([0.6, 0.05, 0.3, 0.25])
        ax_inset.hist(heights_gdf["min_height"], bins=50, color="steelblue")
        for height_threshold in height_thresholds:
            ax_inset.axvline(
                x=height_threshold,
                color="red",
                linestyle="--",
                linewidth=1.5,
                label="Threshold",
            )
        ax_inset.set_title("Distribution", fontsize=9)
        ax_inset.set_xlabel("height", fontsize=8)
        for spine in ["left", "right", "top"]:
            ax_inset.spines[spine].set_visible(False)
        ax_inset.yaxis.set_visible(False)

        return plt.gca()


    @mo.cache
    def make_thresholded_mini_plot(heights_gdf, height_threshold):
        fig, ax = plt.subplots(figsize=(4, 4))
        ax.set_axis_off()
        ax.set_title(f">= {height_threshold}m", fontsize=9)

        plot_boundary(heights_gdf, ax)

        norm = make_norm(heights_gdf)

        thresholded_gdf = heights_gdf[
            heights_gdf["min_height"] >= height_threshold
        ]

        thresholded_gdf.plot("min_height", ax=ax, cmap="terrain", norm=norm)

        return plt.gca()
    return make_header_plot, make_thresholded_mini_plot


@app.cell
def _(mo):
    def grid(items, cols=2):
        """Lay out items in a grid with the specified number of columns."""
        rows = [mo.hstack(items[i : i + cols]) for i in range(0, len(items), cols)]
        return mo.vstack(rows, gap=1)
    return (grid,)


@app.cell
def _(heights_gdf, make_header_plot, mo):
    height_thresholds = [1, 5, 10, 25, 100, 200, 300, 400]
    mo.vstack(
        [
            make_header_plot(heights_gdf, height_thresholds),
        ],
        align="center",
    )
    return (height_thresholds,)


@app.cell
def _(grid, height_thresholds, heights_gdf, make_thresholded_mini_plot):
    grid(
        [make_thresholded_mini_plot(heights_gdf, h) for h in height_thresholds],
        cols=4,
    )
    return


if __name__ == "__main__":
    app.run()
