"""How a replay is laid out when a viewer opens it.

Two maps beside the accuracy each fix reported, over a log of the steps: where the session
went, and what it expected to reach.
"""

import rerun.blueprint as rrb

from .log import ACCURACY, LOG, PREDICTIONS, SAMPLE


def blueprint() -> rrb.Blueprint:
    """The layout a recording carries with it, so it opens as something to read."""
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
