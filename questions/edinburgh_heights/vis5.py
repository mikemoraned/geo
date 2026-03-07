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
    return gpd, griddata, mo, np, plt


@app.cell
def _():
    h3_resolution = 10
    return (h3_resolution,)


@app.cell
def _(gpd, h3_resolution):
    edinburgh_gdf = gpd.read_feather(f"edinburgh_min_{h3_resolution}.arrow")
    return (edinburgh_gdf,)


@app.cell
def _(edinburgh_gdf):
    min_height = edinburgh_gdf[["min_height"]]
    min_height
    return (min_height,)


@app.cell
def _(edinburgh_gdf, griddata, np):
    resolution = 250
    minx, miny, maxx, maxy = edinburgh_gdf.total_bounds

    centroids = edinburgh_gdf.geometry.centroid
    x = centroids.x.values
    y = centroids.y.values
    values = edinburgh_gdf["min_height"].values

    xi = np.linspace(minx, maxx, resolution)
    yi = np.linspace(miny, maxy, resolution)
    grid = griddata((x, y), values, np.meshgrid(xi, yi), method="linear")
    return grid, maxx, maxy, minx, miny


@app.cell
def _(min_height, mo, np):
    (range_min, range_max) = (np.min(min_height), np.max(min_height))
    range_steps = 1000
    range_stepsize = (range_max - range_min) / range_steps
    min_height_slider = mo.ui.slider(start=0, stop=range_steps, step=1)
    return min_height_slider, range_steps, range_stepsize


@app.cell
def _(grid, np, range_steps, range_stepsize):
    height_bins = range(0, range_steps, 1)
    prefiltered = {
        h: np.where(grid > h * range_stepsize, grid, np.nan)
        for h in height_bins
    }
    return (prefiltered,)


@app.cell
def _(min_height_slider, mo, range_stepsize):
    mo.hstack([min_height_slider, mo.md(f"height >= {min_height_slider.value * range_stepsize}m")])
    return


@app.cell
def _(grid, maxx, maxy, min_height_slider, minx, miny, plt, prefiltered):
    fig, (ax1, ax2) = plt.subplots(1, 2, sharex=True, sharey=True, figsize=(16, 8))

    masked = prefiltered[min_height_slider.value]

    ax1.imshow(
        grid, extent=[minx, maxx, miny, maxy], origin="lower", cmap="viridis"
    )
    ax2.imshow(
        masked, extent=[minx, maxx, miny, maxy], origin="lower", cmap="viridis"
    )
    return


if __name__ == "__main__":
    app.run()
