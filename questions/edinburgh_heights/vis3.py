import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import geopandas as gpd
    import numpy as np
    import marimo as mo
    import plotly.express as px
    return gpd, mo, np, px


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
def _(min_height, mo, np):
    (range_min, range_max) = (np.min(min_height), np.max(min_height))
    range_steps = 100
    range_stepsize = (range_max - range_min) / range_steps
    min_height_slider = mo.ui.slider(start=0, stop=range_steps, step=1)
    return min_height_slider, range_steps, range_stepsize


@app.cell
def _(edinburgh_gdf, range_steps, range_stepsize):
    height_bins = range(0, range_steps, 1)
    prefiltered = {
        h: edinburgh_gdf[edinburgh_gdf["min_height"] > h * range_stepsize]
        for h in height_bins
    }
    return


@app.cell
def _(min_height_slider, mo):
    mo.hstack([min_height_slider])
    return


@app.cell
def _():
    # fig, (ax1, ax2) = plt.subplots(1, 2, sharex=True, sharey=True, figsize=(16, 8))
    # ax1.set_aspect("equal")
    # ax2.set_aspect("equal")

    # filtered_gdf = prefiltered[min_height_slider.value]

    # edinburgh_gdf.plot("min_height", ax=ax1)
    # filtered_gdf.plot("min_height", ax=ax2)
    return


@app.cell
def _(edinburgh_gdf, px):
    fig = px.choropleth(edinburgh_gdf, geojson=edinburgh_gdf.geometry.__geo_interface__,
                           locations=edinburgh_gdf.index, color='min_height')
    fig.show()
    return


if __name__ == "__main__":
    app.run()
