import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpd
    import numpy as np
    import marimo as mo
    import matplotlib.pyplot as plt
    from scipy.interpolate import griddata
    from shapely import vectorized
    import contextily as cx
    return cx, gpd, griddata, mo, np, plt, vectorized


@app.cell
def _():
    h3_resolution = 10
    epsg_metres=27700 # a CRS that has unit of metres; use this here to avoid warnings about area calculations
    return (h3_resolution,)


@app.cell
def _(gpd, h3_resolution):
    edinburgh_min_gdf = gpd.read_feather(f"edinburgh_min_{h3_resolution}.arrow").to_crs(epsg=27700).copy()
    return (edinburgh_min_gdf,)


@app.cell
def _(gpd):
    edinburgh_gdf = gpd.read_feather(f"edinburgh.arrow").to_crs(epsg=27700).copy()
    return (edinburgh_gdf,)


@app.cell
def _(edinburgh_min_gdf):
    min_height = edinburgh_min_gdf[["min_height"]]
    min_height
    return (min_height,)


@app.cell
def _(edinburgh_gdf, edinburgh_min_gdf, griddata, np, vectorized):
    resolution = 1000
    minx, miny, maxx, maxy = edinburgh_min_gdf.total_bounds

    centroids = edinburgh_min_gdf.geometry.centroid
    x = centroids.x.values
    y = centroids.y.values
    values = edinburgh_min_gdf["min_height"].values

    xi = np.linspace(minx, maxx, resolution)
    yi = np.linspace(miny, maxy, resolution)

    Xi, Yi = np.meshgrid(xi, yi)
    boundary = edinburgh_gdf.union_all()

    mask = vectorized.contains(boundary, Xi, Yi)
    grid = griddata((x, y), values, (Xi, Yi), method="nearest")
    grid[~mask] = np.nan
    return grid, maxx, maxy, minx, miny


@app.cell
def _(min_height, mo, np):
    (range_min, range_max) = (np.min(min_height), np.max(min_height))
    range_steps = 2000
    range_stepsize = (range_max - range_min) / range_steps
    min_height_slider = mo.ui.slider(start=0, stop=range_steps, step=1)
    return min_height_slider, range_steps, range_stepsize


@app.cell
def _(grid, np, range_steps, range_stepsize):
    height_bins = range(0, range_steps + 1, 1)
    prefiltered = {
        h: np.where(grid > h * range_stepsize, grid, np.nan)
        for h in height_bins
    }
    return (prefiltered,)


@app.cell
def _(cx, grid, maxx, maxy, minx, miny, plt):
    fig1, ax1 = plt.subplots(figsize=(8, 8))
    ax1.imshow(
        grid, extent=[minx, maxx, miny, maxy], origin="lower", cmap="terrain"
    )
    cx.add_basemap(ax1, crs="EPSG:27700", source=cx.providers.CartoDB.Positron, alpha=0.5)

    # plt.tight_layout()
    plt.show()
    return


@app.cell
def _(min_height_slider, mo, range_stepsize):
    mo.hstack([min_height_slider, mo.md(f"height >= {min_height_slider.value * range_stepsize:.1f}m")])
    return


@app.cell
def _(maxx, maxy, min_height_slider, minx, miny, plt, prefiltered):
    fig2, ax2 = plt.subplots(figsize=(8, 8))

    masked = prefiltered[min_height_slider.value]

    ax2.imshow(
        masked, extent=[minx, maxx, miny, maxy], origin="lower", cmap="terrain"
    )

    plt.gca()
    return


if __name__ == "__main__":
    app.run()
