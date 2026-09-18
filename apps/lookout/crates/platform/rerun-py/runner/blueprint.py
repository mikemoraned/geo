import rerun.blueprint as rrb

from .log import ACTUAL_ETA, CROSSINGS, DISTANCE, ERROR, PASSING, PREDICTED_ETA, SESSION

def blueprint() -> rrb.Blueprint:
    return rrb.Blueprint(
        rrb.Vertical(
            rrb.Horizontal(
                rrb.Vertical(
                    rrb.MapView(
                        origin="steps/sample",
                        name="Sample Positions",
                        zoom=16.0,
                        background=rrb.MapProvider.OpenStreetMap
                    ),
                    rrb.MapView(
                        origin="steps/predictions",
                        name="Predictions",
                        zoom=16.0,
                        background=rrb.MapProvider.OpenStreetMap
                    ),
                ),
                rrb.TimeSeriesView(
                    origin="steps/sample/position/accuracy"
                ),
                column_shares=[3,1]
            ),
            rrb.TextLogView(
                origin="steps/log", 
                name="Step Logs"
            ),
            row_shares=[3,1]
        ),
        collapse_panels=True,
    )