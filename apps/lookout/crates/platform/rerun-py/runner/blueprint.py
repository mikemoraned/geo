"""How a replay is laid out when a viewer opens it.

Two halves, because the question has two halves. On the left, where: the track, the crossings
it was scanned against, and which of them it reached. On the right, when: the countdown the
predictor gave against the one the session actually ran, the error between them, and the
distance each prediction was made over.

Nothing here names a crossing. Each plot is the subtree of one measure, so a session with two
crossings and one with two thousand lay out the same way.
"""

import rerun.blueprint as rrb

from .log import ACTUAL_ETA, CROSSINGS, DISTANCE, ERROR, PASSING, PREDICTED_ETA, SESSION


# def blueprint() -> rrb.Blueprint:
#     """The layout a recording carries with it, so it opens as something to read."""
#     return rrb.Blueprint(
#         rrb.Horizontal(
#             rrb.MapView(
#                 name="where",
#                 origin="/",
#                 contents=[f"+ {SESSION}/**", f"+ {CROSSINGS}/**"],
#             ),
#             rrb.Vertical(
#                 rrb.TimeSeriesView(
#                     name="when: predicted against actual",
#                     origin="/",
#                     contents=[f"+ {PREDICTED_ETA}/**", f"+ {ACTUAL_ETA}/**"],
#                 ),
#                 rrb.TimeSeriesView(name="error (seconds)", origin=ERROR),
#                 rrb.TimeSeriesView(name="distance (metres)", origin=DISTANCE),
#                 rrb.TextLogView(name="passings", origin=PASSING),
#             ),
#             column_shares=[3, 2],
#         ),
#         collapse_panels=True,
#     )

def blueprint() -> rrb.Blueprint:
    return rrb.Blueprint(
        rrb.Grid(
            rrb.TextLogView(origin="steps", name="Step Logs"),
            rrb.MapView(
                origin="steps", 
                name="Sample Locations",
                zoom=16.0,
                background=rrb.MapProvider.OpenStreetMap
            )
        ),
        collapse_panels=False,
    )