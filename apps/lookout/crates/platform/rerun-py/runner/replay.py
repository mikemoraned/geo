"""Feeding a session's samples through a predictor, in the order they were recorded."""

from collections.abc import Iterable, Iterator
from dataclasses import dataclass

from lookout_predictor import Prediction

from .store import Sample


@dataclass(frozen=True)
class Step:
    """What the predictor answered at one sample: its predictions, nearest first."""

    sample: Sample
    predictions: list[Prediction]


def replay(predictor, samples: Iterable[Sample]) -> Iterator[Step]:
    """Replays `samples` through `predictor`, a step per sample.

    `predictor` is anything that answers `observe_sample` and `predictions`, built by the
    caller: which predictor to run, and what to give it, is the choice a replay exists to
    compare.

    `samples` arrive oldest first, as `Store.samples` yields them: a fix behind the clock
    raises. Lazy, so a session is replayed as it is drawn rather than collected first, and a
    predictor is left holding the last fix it was given.
    """
    for sample in samples:
        predictor.observe_sample(
            sample.t,
            sample.lat,
            sample.lon,
            altitude_metres=sample.alt,
            speed_mps=sample.speed_mps,
            heading_degrees=sample.heading_degrees,
            accuracy_metres=sample.accuracy_metres,
        )
        yield Step(sample=sample, predictions=predictor.predictions())
