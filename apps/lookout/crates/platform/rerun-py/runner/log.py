from collections.abc import Iterable
from datetime import datetime

import rerun as rr

from .replay import Step
from .store import Passing

TIMELINE = "t"

SESSION = "session"
TRACK = f"{SESSION}/track"
FIX = f"{SESSION}/fix"
CROSSINGS = "crossings"
PASSED = "crossings/passed"

# Grouped by what is measured rather than by which crossing it is measured against, so one
# plot is one subtree and a crossing is a series within it.
DISTANCE = "distance"
PREDICTED_ETA = "eta/predicted"
ACTUAL_ETA = "eta/actual"
ERROR = "error"
PASSING = "passing"


def error_seconds(predicted_at: datetime | None, passed_at: datetime | None) -> float | None:
    """How wrong a predicted arrival was, in seconds, positive where it was late.

    `None` where either instant is missing: a prediction carries no time when there is no
    speed to divide by, and a crossing the session never passed has nothing to be wrong
    against.
    """
    if predicted_at is None or passed_at is None:
        return None
    return (predicted_at - passed_at).total_seconds()


def draw(
    recording: rr.RecordingStream,
    steps: Iterable[Step],
    crossings: Iterable[tuple[int, float, float]],
    passings: Iterable[Passing],
) -> None:
    crossing_locations = {crossing: (lat, lon) for crossing, lat, lon in crossings}
    
    recording.log(
        "steps/sample/position/accuracy",
        rr.SeriesPoints(
            colors=[255, 0, 0],
            names="accuracy",
            markers="circle",
            marker_sizes=4,
        ),
        static=True,
    )
    positions = []
    max_distance_metres = 1000 # 1km
    for step_index, step in enumerate(steps):
        t = step.sample.t
        recording.set_time(TIMELINE, timestamp=t)
        recording.log(f"steps/log", rr.TextLog(f"{step_index}: Step", level=rr.TextLogLevel.INFO))
        sample = step.sample
        position = (sample.lat, sample.lon)
        positions.append(position)
        recording.log(f"steps/sample/position",
                      rr.GeoPoints(lat_lon=[position], radii=rr.Radius.ui_points(10.0)))
        recording.log(f"steps/sample/positions",
                      rr.GeoLineStrings(lat_lon=positions, radii=rr.Radius.ui_points(2.0)))
        if sample.accuracy_metres:
            recording.log(f"steps/sample/position/accuracy", rr.Scalars(sample.accuracy_metres))
        predicted_crossings = []
        for prediction in step.predictions:
            crossing = crossing_locations.get(prediction.crossing)
            if crossing and prediction.metres < max_distance_metres:
                print(f"{prediction.crossing}->{crossing}: {prediction.metres}")
                predicted_crossings.append(crossing)
        if len(predicted_crossings) > 0:
            recording.log(f"steps/log", rr.TextLog(f"{step_index}: found {len(predicted_crossings)} within {max_distance_metres}metres", level=rr.TextLogLevel.INFO))
            recording.log(f"steps/predictions",
                        rr.GeoPoints(lat_lon=predicted_crossings, radii=rr.Radius.ui_points(5.0)))
            for predicted_crossing in predicted_crossings:
                recording.log(f"steps/predictions", rr.GeoLineStrings(lat_lon=[position,predicted_crossing], radii=rr.Radius.ui_points(2.0)))
        
