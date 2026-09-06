"""Drawing a replay into a rerun recording.

What a reader is meant to see: the track the session took, the crossings it was scanned
against, and — as each fix is taken — how far the predictor thought each crossing was and how
wrong its arrival time was. The error is what the recording exists for, so it is a series
against the clock rather than one number at the end: a prediction that converges on the water
and one that never does look nothing alike over a run.

What to draw and drawing it are separate. `drawings` decides the entity paths, the instants
and the values, and answers them as data a test can read; `draw` turns each into the rerun
archetype it is logged as. Nothing about rerun reaches the first, and no decision reaches the
second.
"""

from collections.abc import Iterable, Iterator, Mapping
from dataclasses import dataclass
from datetime import datetime

import rerun as rr

from .replay import Step

TIMELINE = "t"

TRACK = "session/track"
FIX = "session/fix"
CROSSINGS = "crossings"


@dataclass(frozen=True)
class Marker:
    """Places on the map, at `at` — or for the whole recording where that is `None`."""

    path: str
    lat_lon: list[tuple[float, float]]
    at: datetime | None = None


@dataclass(frozen=True)
class Line:
    """A path across the map, through `lat_lon` in order."""

    path: str
    lat_lon: list[tuple[float, float]]
    at: datetime | None = None


@dataclass(frozen=True)
class Measure:
    """A number against the clock, one point of a series."""

    path: str
    value: float
    at: datetime | None = None


Drawing = Marker | Line | Measure


def error_seconds(predicted_at: datetime | None, passed_at: datetime | None) -> float | None:
    """How wrong a predicted arrival was, in seconds, positive where it was late.

    `None` where either instant is missing: a prediction carries no time when there is no
    speed to divide by, and a crossing the session never passed has nothing to be wrong
    against.
    """
    if predicted_at is None or passed_at is None:
        return None
    return (predicted_at - passed_at).total_seconds()


def drawings(
    steps: Iterable[Step],
    crossings: Iterable[tuple[int, float, float]],
    passed: Mapping[int, datetime],
) -> Iterator[Drawing]:
    """What a replay draws, in the order it is drawn.

    `crossings` are those the predictor scans, drawn once as the map it scans them on.
    `passed` says when the session actually reached each of them, which is what an error is
    measured against; a crossing missing from it is drawn without one.

    The track comes last, because it is the whole of the run rather than a step of it.
    """
    yield Marker(CROSSINGS, [(lat, lon) for _, lat, lon in crossings])

    track: list[tuple[float, float]] = []
    for step in steps:
        at = step.sample.t
        track.append((step.sample.lat, step.sample.lon))
        yield Marker(FIX, [track[-1]], at)

        for prediction in step.predictions:
            path = f"predicted/{prediction.crossing}"
            yield Measure(f"{path}/metres", prediction.metres, at)
            error = error_seconds(prediction.at, passed.get(prediction.crossing))
            if error is not None:
                yield Measure(f"{path}/error_seconds", error, at)

    yield Line(TRACK, track)


def draw(recording: rr.RecordingStream, drawn: Iterable[Drawing]) -> None:
    """Logs each drawing to `recording`, on a timeline of the fixes' own instants.

    One without an instant is logged for the whole recording rather than at a point in it.
    """
    for drawing in drawn:
        if drawing.at is not None:
            recording.set_time(TIMELINE, timestamp=drawing.at)

        match drawing:
            case Marker(path, lat_lon, at):
                recording.log(path, rr.GeoPoints(lat_lon=lat_lon), static=at is None)
            case Line(path, lat_lon, at):
                recording.log(path, rr.GeoLineStrings(lat_lon=[lat_lon]), static=at is None)
            case Measure(path, value, at):
                recording.log(path, rr.Scalars(value), static=at is None)
