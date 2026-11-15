# /// script
# dependencies = [
#     "lonboard==0.16.0",
#     "lookout-medallion",
#     "marimo>=0.25.2",
#     "polars==2.1.0",
#     "pyarrow==25.0.0",
#     "shapely==2.1.2",
# ]
#
# [tool.uv.sources]
# lookout-medallion = { path = "../../crates/medallion-py" }
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import lonboard
    import marimo as mo
    import polars as pl
    import pyarrow as pa
    import shapely

    import lookout_medallion

    MEDALLION_ROOT = str(lookout_medallion.default_root())
    COUNTRIES = lookout_medallion.countries_of(
        "water_crossing_v2", root=MEDALLION_ROOT
    )

    COUNTRIES
    return (
        COUNTRIES,
        MEDALLION_ROOT,
        lonboard,
        lookout_medallion,
        mo,
        pa,
        pl,
        shapely,
    )


@app.cell
def _(mo):
    mo.md(r"""
    # The base map's size

    The kiosk's base map draws whole the rail segments and water features that `water_crossing_v2`
    names. This notebook counts those features in each country, and measures their WKB in the extract
    each crossing came from, before the map's zoom levels are chosen.
    """)
    return


@app.cell
def _(MEDALLION_ROOT, lookout_medallion, pa, pl):
    def silver(sql: str, country: str) -> pl.DataFrame:
        return pl.from_arrow(
            pa.table(
                lookout_medallion.query_silver(
                    sql, country=country, root=MEDALLION_ROOT
                )
            )
        ).with_columns(country=pl.lit(country))

    def bronze(sql: str, **named) -> pl.DataFrame:
        return pl.from_arrow(
            pa.table(
                lookout_medallion.query_bronze(
                    sql, root=MEDALLION_ROOT, **named
                )
            )
        )

    return bronze, silver


@app.cell
def _(COUNTRIES, pl, silver):
    crossings = pl.concat(
        [
            silver(
                """
                SELECT crossing_id, extract_id,
                       overture_rail_id, overture_rail_class,
                       overture_water_id, overture_water_subtype, overture_water_class
                FROM water_crossing_v2
                """,
                country,
            )
            for country in COUNTRIES
        ]
    )

    crossings.group_by("country", "extract_id").agg(
        crossings=pl.len(),
        rail=pl.col("overture_rail_id").n_unique(),
        water=pl.col("overture_water_id").n_unique(),
    ).sort("country")
    return (crossings,)


@app.cell
def _(mo):
    mo.md(r"""
    A feature named in two countries would land in two partitions of `rail_v1` or `water_v1`, and be
    drawn twice. Both counts are 0 when no id appears in more than one country.
    """)
    return


@app.cell
def _(crossings, pl):
    pl.DataFrame(
        {
            id_column: [
                crossings.group_by(id_column)
                .agg(pl.col("country").n_unique())
                .filter(pl.col("country") > 1)
                .height
            ]
            for id_column in ["overture_rail_id", "overture_water_id"]
        }
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    Each named feature is read from the extract its crossings came from. Every named id is found when
    `named` and `found` agree.
    """)
    return


@app.cell
def _(bronze, crossings, pl):
    FEATURE_TABLES = {
        "rail": (
            "overture_rail_id",
            {"theme": "transportation", "type": "segment"},
        ),
        "water": ("overture_water_id", {"theme": "base", "type": "water"}),
    }

    def named_features(kind: str) -> pl.DataFrame:
        id_column, table = FEATURE_TABLES[kind]
        named = crossings.select("country", "extract_id", id_column).unique()
        return pl.concat(
            [
                bronze(
                    f"""
                    SELECT id, subtype, class, geometry FROM feature
                    WHERE extract_id = $extract_id
                      AND id IN ({", ".join(f"'{id}'" for id in of_extract[id_column])})
                    """,
                    tables={"feature": table},
                    params={"extract_id": extract_id},
                ).with_columns(kind=pl.lit(kind), country=pl.lit(country))
                for (country, extract_id), of_extract in named.group_by(
                    "country", "extract_id"
                )
            ]
        )

    features = pl.concat([named_features(kind) for kind in FEATURE_TABLES])

    features.group_by("kind", "country").agg(found=pl.len()).join(
        pl.concat(
            [
                crossings.group_by("country")
                .agg(named=pl.col(id_column).n_unique())
                .with_columns(kind=pl.lit(kind))
                for kind, (id_column, _) in FEATURE_TABLES.items()
            ]
        ),
        on=["kind", "country"],
    ).sort("kind", "country")
    return (features,)


@app.cell
def _(mo):
    mo.md(r"""
    The WKB each kind takes per country, and its largest single feature.
    """)
    return


@app.cell
def _(features, pl):
    features_sized = features.with_columns(
        wkb_bytes=pl.col("geometry").bin.size()
    )

    features_sized.group_by("kind", "country").agg(
        features=pl.len(),
        wkb_mb=(pl.col("wkb_bytes").sum() / 1e6).round(1),
        largest_mb=(pl.col("wkb_bytes").max() / 1e6).round(2),
    ).sort("kind", "country")
    return (features_sized,)


@app.cell
def _(mo):
    mo.md(r"""
    Water's bytes by what the water is, largest first.
    """)
    return


@app.cell
def _(features_sized, pl):
    (
        features_sized.filter(pl.col("kind") == "water")
        .group_by("country", "subtype", "class")
        .agg(
            features=pl.len(),
            wkb_mb=(pl.col("wkb_bytes").sum() / 1e6).round(2),
        )
        .sort("wkb_mb", descending=True)
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    ## Clipped to each country

    Each extract holds two areas for its country: `land`, and `maritime`, which adds its territorial
    waters. Clipping the named features to either one cuts away whatever lies beyond it. SedonaDB
    clips through its GEOS backend, which the workspace leaves out, so the clip runs in shapely on the
    WKB read through the binding.
    """)
    return


@app.cell
def _(bronze, crossings, pl):
    country_areas = bronze(
        f"""
        SELECT country, class, geometry FROM area
        WHERE subtype = 'country'
          AND extract_id IN ({", ".join(f"'{id}'" for id in crossings["extract_id"].unique())})
        """,
        tables={"area": {"theme": "divisions", "type": "division_area"}},
    )

    country_areas.select(
        "country",
        "class",
        wkb_mb=(pl.col("geometry").bin.size() / 1e6).round(2),
    ).sort("country", "class")
    return (country_areas,)


@app.cell
def _(country_areas, features, lonboard, pl, shapely):
    lonboard.viz(
        [
            shapely.from_wkb(
                country_areas.filter(pl.col("class") == "maritime")[
                    "geometry"
                ].to_numpy()
            ),
            shapely.from_wkb(
                country_areas.filter(pl.col("class") == "land")[
                    "geometry"
                ].to_numpy()
            ),
            shapely.from_wkb(
                features.filter(pl.col("kind") == "water")[
                    "geometry"
                ].to_numpy()
            ),
        ]
    )
    return


@app.cell
def _(COUNTRIES, country_areas, features_sized, pl, shapely):
    def clipped_to(country: str, boundary: str) -> pl.DataFrame:
        _area = shapely.from_wkb(
            country_areas.filter(
                (pl.col("country") == country) & (pl.col("class") == boundary)
            )["geometry"].item()
        )
        shapely.prepare(_area)
        _of_country = features_sized.filter(pl.col("country") == country)
        _clipped = shapely.intersection(
            shapely.from_wkb(_of_country["geometry"].to_numpy()), _area
        )
        return _of_country.select(
            "kind", "country", "id", "subtype", "class", "wkb_bytes"
        ).with_columns(
            boundary=pl.lit(boundary),
            clipped=pl.Series(
                shapely.to_wkb(_clipped).tolist(), dtype=pl.Binary
            ),
            clipped_empty=pl.Series(shapely.is_empty(_clipped).tolist()),
        )

    clipped = pl.concat(
        [
            clipped_to(country, boundary)
            for country in COUNTRIES
            for boundary in ["land", "maritime"]
        ]
    ).with_columns(clipped_bytes=pl.col("clipped").bin.size())

    clipped.group_by("kind", "country", "boundary").agg(
        features=pl.len(),
        emptied=pl.col("clipped_empty").sum(),
        wkb_mb=(pl.col("wkb_bytes").sum() / 1e6).round(1),
        clipped_mb=(pl.col("clipped_bytes").sum() / 1e6).round(1),
    ).sort("kind", "country", "boundary")
    return (clipped,)


@app.cell
def _(mo):
    mo.md(r"""
    Water's bytes by what the water is, before and after clipping to the land area, largest first.
    """)
    return


@app.cell
def _(clipped, pl):
    (
        clipped.filter(
            (pl.col("kind") == "water") & (pl.col("boundary") == "land")
        )
        .group_by("country", "subtype", "class")
        .agg(
            features=pl.len(),
            emptied=pl.col("clipped_empty").sum(),
            wkb_mb=(pl.col("wkb_bytes").sum() / 1e6).round(2),
            clipped_mb=(pl.col("clipped_bytes").sum() / 1e6).round(2),
        )
        .sort("wkb_mb", descending=True)
    )
    return


@app.cell
def _(mo):
    mo.md(r"""
    The largest water feature, as named and as clipped to the land area.
    """)
    return


@app.cell
def _(clipped, features, lonboard, pl, shapely):
    def _polygons_of(wkb: pl.Series):
        _parts = shapely.get_parts(shapely.from_wkb(wkb.to_numpy()))
        return _parts[
            shapely.get_type_id(_parts) == shapely.GeometryType.POLYGON
        ]

    _largest = (
        clipped.filter(
            (pl.col("kind") == "water") & (pl.col("boundary") == "land")
        )
        .sort("wkb_bytes", descending=True)
        .head(1)
    )

    lonboard.viz(
        [
            _polygons_of(
                features.filter(pl.col("id") == _largest["id"].item())[
                    "geometry"
                ]
            ),
            _polygons_of(_largest["clipped"]),
        ]
    )
    return


if __name__ == "__main__":
    app.run()
