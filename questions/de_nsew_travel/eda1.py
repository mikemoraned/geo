import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import geopandas as gpt
    import overturemaps
    import duckdb
    return (overturemaps,)


@app.cell
def _(overturemaps):
    div = overturemaps.core.geodataframe(overture_type='division')
    div.head()
    return (div,)


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
    cities_de = de_div[de_div['subtype'] == 'locality'].sort_values('population', ascending=False)
    cities_de[['names', 'population', 'geometry']].head(20)
    return (cities_de,)


@app.cell
def _(cities_de):
    cities_de[['names', 'population', 'geometry']].explore("population")
    return


if __name__ == "__main__":
    app.run()
