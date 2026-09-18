"""Drawing a replay into a rerun recording.

What a reader is meant to see: the track the session took, the crossings it was scanned
against, and — as each fix is taken — how far the predictor thought each crossing was and how
wrong its arrival time was. The error is what the recording exists for, so it is a series
against the clock rather than one number at the end: a prediction that converges on the water
and one that never does look nothing alike over a run.

Every entity is logged through the recording it is given, so what is drawn can be read back by
standing in for one. Nothing here reaches for the recording rerun holds globally.
"""

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
        


    # next: show positions of predicted passings at each time

    # """Draws a replay, on a timeline of the fixes' own instants.

    # `crossings` are those the predictor scans, drawn once as the map it scans them on.
    # `passings` are the ground truth: which of them the session actually reached and when,
    # which is what a predicted time is compared against. A crossing the session never reached
    # is drawn as a prediction with nothing to answer to.

    # What holds for the whole recording — the map, the track — is logged as static rather than
    # at an instant of it.
    # """
    # crossings = list(crossings)
    # passed = {passing.crossing: passing for passing in passings}
    # where = {crossing: (lat, lon) for crossing, lat, lon in crossings}

    # recording.log(
    #     CROSSINGS, rr.GeoPoints(lat_lon=[(lat, lon) for _, lat, lon in crossings]), static=True
    # )
    # recording.log(
    #     PASSED,
    #     rr.GeoPoints(lat_lon=[where[crossing] for crossing in passed if crossing in where]),
    #     static=True,
    # )

    # for passing in passed.values():
    #     recording.set_time(TIMELINE, timestamp=passing.at)
    #     recording.log(
    #         f"{PASSING}/{passing.crossing}",
    #         rr.TextLog(f"passed, nearest sample {passing.distance_metres:.0f}m away"),
    #     )

    # track: list[tuple[float, float]] = []
    # for step in steps:
    #     at = step.sample.t
    #     recording.set_time(TIMELINE, timestamp=at)
    #     track.append((step.sample.lat, step.sample.lon))
    #     recording.log(FIX, rr.GeoPoints(lat_lon=[track[-1]]))

    #     for prediction in step.predictions:
    #         crossing = prediction.crossing
    #         recording.log(f"{DISTANCE}/{crossing}", rr.Scalars(prediction.metres))

    #         if prediction.at is not None:
    #             recording.log(
    #                 f"{PREDICTED_ETA}/{crossing}",
    #                 rr.Scalars((prediction.at - at).total_seconds()),
    #             )

    #         passing = passed.get(crossing)
    #         if passing is not None:
    #             # The same countdown the prediction is guessing at, so the two lie on one
    #             # plot and the gap between them is the error.
    #             recording.log(
    #                 f"{ACTUAL_ETA}/{crossing}", rr.Scalars((passing.at - at).total_seconds())
    #             )

    #         error = error_seconds(prediction.at, passing.at if passing else None)
    #         if error is not None:
    #             recording.log(f"{ERROR}/{crossing}", rr.Scalars(error))

    # # The track is the whole of the run rather than a step of it, so it is drawn once every
    # # step has been taken.
    # recording.log(TRACK, rr.GeoLineStrings(lat_lon=[track]), static=True)
