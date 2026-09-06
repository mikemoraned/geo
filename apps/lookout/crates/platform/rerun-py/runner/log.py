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

from collections.abc import Iterable, Iterator
from dataclasses import dataclass
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


@dataclass(frozen=True)
class Note:
    """A line of text at a moment, for something that happens rather than something that
    holds a value."""

    path: str
    text: str
    at: datetime | None = None


Drawing = Marker | Line | Measure | Note


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
    passings: Iterable[Passing],
) -> Iterator[Drawing]:
    """What a replay draws, in the order it is drawn.

    `crossings` are those the predictor scans, drawn once as the map it scans them on.
    `passings` are the ground truth: which of them the session actually reached and when,
    which is what a predicted time is compared against. A crossing the session never reached
    is drawn as a prediction with nothing to answer to.

    The track comes last, because it is the whole of the run rather than a step of it.
    """
    crossings = list(crossings)
    passed = {passing.crossing: passing for passing in passings}
    where = {crossing: (lat, lon) for crossing, lat, lon in crossings}

    yield Marker(CROSSINGS, [(lat, lon) for _, lat, lon in crossings])
    yield Marker(PASSED, [where[crossing] for crossing in passed if crossing in where])

    for passing in passed.values():
        yield Note(
            f"{PASSING}/{passing.crossing}",
            f"passed, nearest sample {passing.distance_metres:.0f}m away",
            passing.at,
        )

    track: list[tuple[float, float]] = []
    for step in steps:
        at = step.sample.t
        track.append((step.sample.lat, step.sample.lon))
        yield Marker(FIX, [track[-1]], at)

        for prediction in step.predictions:
            crossing = prediction.crossing
            yield Measure(f"{DISTANCE}/{crossing}", prediction.metres, at)

            if prediction.at is not None:
                yield Measure(
                    f"{PREDICTED_ETA}/{crossing}", (prediction.at - at).total_seconds(), at
                )

            passing = passed.get(crossing)
            if passing is not None:
                # The same countdown the prediction is guessing at, so the two lie on one
                # plot and the gap between them is the error.
                yield Measure(
                    f"{ACTUAL_ETA}/{crossing}", (passing.at - at).total_seconds(), at
                )

            error = error_seconds(prediction.at, passing.at if passing else None)
            if error is not None:
                yield Measure(f"{ERROR}/{crossing}", error, at)

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
            case Note(path, text, at):
                recording.log(path, rr.TextLog(text), static=at is None)
