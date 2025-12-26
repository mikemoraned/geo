import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import geopandas as gpt
    import pandas as pd
    import overturemaps
    import duckdb
    return (overturemaps,)


@app.cell
def _(overturemaps):
    div = overturemaps.core.geodataframe(overture_type='division')
    div = div.set_crs('EPSG:4326')
    div.head()
    return (div,)


@app.cell
def _(div):
    div.crs
    return


@app.cell
def _(div):
    de_div = div[div['country'] == 'DE']
    de_div.head()
    return (de_div,)


@app.cell
def _(de_div):
    de_div[de_div['population'] > 0].head()
    return


@app.cell
def _(de_div):
    cities_de = de_div[de_div['class'] == 'city'].sort_values('population', ascending=False)
    cities_de[['names', 'population', 'geometry']].head(20)
    return (cities_de,)


@app.cell
def _():
    # cities_de[['names', 'population', 'geometry']].explore("population")
    return


@app.cell
def _():
    from lonboard import Map, ScatterplotLayer
    import pyarrow
    return Map, ScatterplotLayer


@app.cell
def _(Map, ScatterplotLayer, cities_de):
    Map(ScatterplotLayer.from_geopandas(
        cities_de,
        get_fill_color=[255, 0, 128, 200],
        get_radius=cities_de['population'].fillna(0) / 1000,  # Scale population for visible radius
        radius_min_pixels=2,
        radius_max_pixels=50,
        pickable=True,
    ))
    return


@app.cell
def _(cities_de):
    # Calculate the 75th percentile threshold
    top_quartile_threshold = cities_de['population'].quantile(0.75)

    # Filter to only include cities in the top quartile
    cities_de_top_quartile = cities_de[cities_de['population'] >= top_quartile_threshold]
    cities_de_top_quartile[['names', 'population', 'geometry']].head(20)
    return (cities_de_top_quartile,)


@app.cell
def _(Map, ScatterplotLayer, cities_de_top_quartile):
    import numpy as np

    Map(ScatterplotLayer.from_geopandas(
        cities_de_top_quartile[['id','names', 'population', 'geometry']],
        get_fill_color=[255, 0, 128, 200],
        get_radius=np.log10(cities_de_top_quartile['population'].fillna(1).replace(0, 1)) * 100,  # Scale log10 of population for visible radius
        radius_min_pixels=2,
        radius_max_pixels=50,
        pickable=True,
    ))
    return


@app.cell
def _(cities_de_top_quartile):
    cities_de_top_quartile[cities_de_top_quartile['id'] == '9187e609-5a2f-4535-85ec-e2b88399eea3']
    return


if __name__ == "__main__":
    app.run()
