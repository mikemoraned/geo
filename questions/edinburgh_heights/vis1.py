import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpd
    import numpy as np
    import marimo as mo
    import matplotlib.pyplot as plt
    return gpd, mo, np, plt


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
    edinburgh_gdf.plot("min_height")
    return


@app.cell
def _(edinburgh_gdf):
    min_height = edinburgh_gdf[["min_height"]]
    min_height
    return (min_height,)


@app.cell
def _(min_height, mo, np):
    (range_min, range_max) = (np.min(min_height), np.max(min_height))
    range_steps = 100
    range_stepsize = (range_max - range_min) / range_steps
    min_height_slider = mo.ui.slider(
        start=range_min, stop=range_max, step=range_stepsize
    )
    return (min_height_slider,)


@app.cell
def _(min_height_slider, mo):
    mo.hstack([min_height_slider])
    return


@app.cell
def _(edinburgh_gdf, min_height_slider, plt):
    fig, (ax1, ax2) = plt.subplots(1, 2, sharex=True, sharey=True, figsize=(16, 8))
    ax1.set_aspect("equal")
    ax2.set_aspect("equal")

    edinburgh_gdf.plot("min_height", ax=ax1)
    edinburgh_gdf[edinburgh_gdf["min_height"] > min_height_slider.value].plot(
        "min_height", ax=ax2
    )


    return


if __name__ == "__main__":
    app.run()
