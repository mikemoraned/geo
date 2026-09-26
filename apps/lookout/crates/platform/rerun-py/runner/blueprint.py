import rerun.blueprint as rrb

from .log import ACCURACY, LOG, PREDICTIONS, SAMPLE


def blueprint() -> rrb.Blueprint:
    return rrb.Blueprint(
        rrb.Vertical(
            rrb.Horizontal(
                rrb.Vertical(
                    rrb.MapView(
                        origin=SAMPLE,
                        name="Sample Positions",
                        zoom=16.0,
                        background=rrb.MapProvider.OpenStreetMap,
                    ),
                    rrb.MapView(
                        origin=PREDICTIONS,
                        name="Predictions",
                        zoom=16.0,
                        background=rrb.MapProvider.OpenStreetMap,
                    ),
                ),
                rrb.TimeSeriesView(origin=ACCURACY),
                column_shares=[3, 1],
            ),
            rrb.TextLogView(origin=LOG, name="Step Logs"),
            row_shares=[3, 1],
        ),
        collapse_panels=True,
    )
