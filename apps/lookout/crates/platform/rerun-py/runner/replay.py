from collections.abc import Iterable, Iterator
from dataclasses import dataclass

from lookout_predictor import Prediction

from .store import Sample


@dataclass(frozen=True)
class Step:
    sample: Sample
    predictions: list[Prediction]


def replay(predictor, samples: Iterable[Sample]) -> Iterator[Step]:
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
