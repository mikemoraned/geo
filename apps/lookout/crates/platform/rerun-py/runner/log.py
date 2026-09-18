"""Drawing a replay into a rerun recording.
"""

from collections.abc import Iterable

import rerun as rr

from .replay import Step

TIMELINE = "t"

LOG = "steps/log"
SAMPLE = "steps/sample"
POSITION = f"{SAMPLE}/position"
POSITIONS = f"{SAMPLE}/positions"
ACCURACY = f"{POSITION}/accuracy"
PREDICTIONS = "steps/predictions"

NEAR_METRES = 1_000.0


def draw(
    recording: rr.RecordingStream,
    steps: Iterable[Step],
    crossings: Iterable[tuple[int, float, float]],
) -> None:
    where = {crossing: (lat, lon) for crossing, lat, lon in crossings}

    recording.log(
        ACCURACY,
        rr.SeriesPoints(colors=[255, 0, 0], names="accuracy", markers="circle", marker_sizes=4),
        static=True,
    )

    positions: list[tuple[float, float]] = []
    for index, step in enumerate(steps):
        sample = step.sample
        position = (sample.lat, sample.lon)
        positions.append(position)

        recording.set_time(TIMELINE, timestamp=sample.t)
        recording.log(LOG, rr.TextLog(f"{index}: Step", level=rr.TextLogLevel.INFO))
        recording.log(
            POSITION, rr.GeoPoints(lat_lon=[position], radii=rr.Radius.ui_points(10.0))
        )
        recording.log(
            POSITIONS, rr.GeoLineStrings(lat_lon=positions, radii=rr.Radius.ui_points(2.0))
        )

        if sample.accuracy_metres is not None:
            recording.log(ACCURACY, rr.Scalars(sample.accuracy_metres))

        near = [
            where[prediction.crossing]
            for prediction in step.predictions
            if prediction.crossing in where and prediction.metres < NEAR_METRES
        ]
        if near:
            recording.log(
                LOG,
                rr.TextLog(
                    f"{index}: found {len(near)} within {NEAR_METRES:.0f}metres",
                    level=rr.TextLogLevel.INFO,
                ),
            )
            recording.log(
                PREDICTIONS, rr.GeoPoints(lat_lon=near, radii=rr.Radius.ui_points(5.0))
            )
            for crossing in near:
                recording.log(
                    PREDICTIONS,
                    rr.GeoLineStrings(
                        lat_lon=[position, crossing], radii=rr.Radius.ui_points(2.0)
                    ),
                )
