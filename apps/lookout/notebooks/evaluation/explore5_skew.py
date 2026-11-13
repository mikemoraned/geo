# /// script
# dependencies = ["marimo"]
# requires-python = ">=3.13"
# ///

import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def imports():
    import geopandas as gpd
    import lonboard
    import marimo as mo
    import matplotlib.pyplot as plt
    import pyarrow as pa

    import lookout_medallion

    return gpd, lonboard, lookout_medallion, mo, pa, plt


@app.cell
def intro(mo):
    mo.md("""
    # Skew of the online pass detector

    Ground truth marks a pass where a session's nearest sample comes within its match radius of a
    crossing. The apps see only the latest sample, so they can see a pass late, early, or not at all.
    **Skew** measures that gap. It describes the detector and the data, not the predictor.

    A **visit** is a run of samples within the online pass radius of one crossing. A pass is
    **seen** when a visit of its crossing spans it. A visit is **unmatched** when it spans no pass.
    The **offset** is how long before or after its pass a visit starts.
    """)
    return


@app.cell
def skew_runs(lookout_medallion, pa):
    skew_runs = pa.table(
        lookout_medallion.query_gold(
            "SELECT run.run_id, run.predictor, run.sessions, "
            "CAST(run.online_detector_settings[arrow_cast('pass_radius_metres', 'LargeUtf8')] AS DOUBLE) "
            "AS pass_radius_metres, "
            "skew.passes, skew.seen_passes, skew.seen, skew.seen_low, skew.seen_high, "
            "skew.visits, skew.unmatched_visits, skew.unmatched, skew.unmatched_low, "
            "skew.unmatched_high, skew.offset_p10_ms, skew.offset_median_ms, skew.offset_p90_ms "
            "FROM evaluation_run_v4 AS run JOIN evaluation_skew_v1 AS skew USING (run_id) "
            "ORDER BY run.sessions DESC, pass_radius_metres"
        )
    ).to_pandas()
    skew_runs
    return (skew_runs,)


@app.cell
def run_choice(mo, skew_runs):
    run_choice = mo.ui.dropdown(
        options={
            f"{_run.pass_radius_metres:g} m over {_run.sessions:,} sessions": _run.run_id
            for _run in skew_runs.itertuples()
        },
        value=f"100 m over {skew_runs.sessions.max():,} sessions",
        label="Skew run",
    )
    run_choice
    return (run_choice,)


@app.cell
def run(mo, run_choice, skew_runs):
    run = skew_runs.set_index("run_id").loc[run_choice.value]
    mo.md(
        f"Run `{run_choice.value}`: online pass radius {run.pass_radius_metres:g} m, "
        f"{run.seen_passes:,} of {run.passes:,} passes seen"
    )
    return (run,)


@app.cell
def run_sessions(lookout_medallion, mo, pa, run_choice):
    run_sessions = pa.table(
        lookout_medallion.query_gold(
            "SELECT session_id FROM evaluation_session_v1 WHERE run_id = $run_id",
            params={"run_id": run_choice.value},
        )
    ).to_pandas()
    mo.md(
        f"{len(run_sessions):,} sessions in the run, {run_sessions.session_id.nunique():,} distinct"
    )
    return (run_sessions,)


@app.cell
def ground_truth(lookout_medallion, pa):
    ground_truth = pa.table(
        lookout_medallion.query_silver(
            "SELECT session_id, crossing_id, crossed_at, distance_m, match_radius_m "
            "FROM train_session_crossing_v1"
        )
    ).to_pandas()
    ground_truth.groupby("match_radius_m").size().rename("passes")
    return (ground_truth,)


@app.cell
def passes(ground_truth, mo, run, run_sessions):
    passes = ground_truth[
        ground_truth.session_id.isin(run_sessions.session_id)
    ]
    mo.md(
        f"{len(passes):,} of {len(ground_truth):,} ground-truth passes fall in the run's sessions, "
        f"against the {run.passes:,} passes the skew counted"
    )
    return (passes,)


@app.cell
def unseen_heading(mo):
    mo.md("""
    ## Passes the chosen run cannot see

    Each crossing is sized and coloured by how many of its passes lie beyond the chosen run's
    online pass radius.
    """)
    return


@app.cell
def crossings(gpd, lookout_medallion, mo, pa):
    crossings = gpd.GeoDataFrame.from_arrow(
        pa.table(
            lookout_medallion.query_silver(
                "SELECT crossing_id, geometry FROM water_crossing_v2",
                country="DE",
            )
        )
    )
    mo.md(f"{len(crossings):,} crossings in DE")
    return (crossings,)


@app.cell
def unseen_counts(mo, passes, run):
    unseen_counts = (
        passes[passes.distance_m > run.pass_radius_metres]
        .groupby("crossing_id")
        .agg(unseen=("distance_m", "size"), median_m=("distance_m", "median"))
        .reset_index()
    )
    mo.md(
        f"{unseen_counts.unseen.sum():,} passes beyond {run.pass_radius_metres:g} m, "
        f"on {len(unseen_counts):,} crossings"
    )
    return (unseen_counts,)


@app.cell
def unseen_crossings(crossings, mo, unseen_counts):
    unseen_crossings = crossings.merge(
        unseen_counts, on="crossing_id", validate="one_to_one"
    ).sort_values("unseen", ascending=False)
    mo.vstack(
        [
            mo.md(
                f"{len(unseen_crossings):,} of {len(unseen_counts):,} crossings "
                "found their place"
            ),
            unseen_crossings.drop(columns="geometry").head(10),
        ]
    )
    return (unseen_crossings,)


@app.cell
def unseen_map(lonboard, plt, unseen_crossings):
    lonboard.Map(
        lonboard.ScatterplotLayer.from_geopandas(
            unseen_crossings[
                ["crossing_id", "unseen", "median_m", "geometry"]
            ],
            get_fill_color=lonboard.colormap.apply_continuous_cmap(
                (
                    unseen_crossings.unseen / unseen_crossings.unseen.max()
                ).to_numpy(),
                plt.get_cmap("plasma"),
            ),
            get_radius=(unseen_crossings.unseen.pow(0.5) * 30).to_numpy(),
            radius_min_pixels=2,
        ),
        basemap=lonboard.basemap.MaplibreBasemap(
            style=lonboard.basemap.CartoStyle.Positron
        ),
    )
    return


if __name__ == "__main__":
    app.run()
