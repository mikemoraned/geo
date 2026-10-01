from collections.abc import Iterator
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

import lookout_medallion
import pyarrow as pa


@dataclass(frozen=True)
class Session:
    session_id: str
    country: str
    first: datetime
    last: datetime
    samples: int


@dataclass(frozen=True)
class Sample:
    t: datetime
    lat: float
    lon: float
    alt: float | None
    accuracy_metres: float | None
    speed_mps: float | None
    heading_degrees: float | None


class Store:
    def __init__(self, root: Path | None = None) -> None:
        self.root = None if root is None else str(root)

    def _query(self, sql: str, country: str | None = None, **params) -> pa.Table:
        return pa.table(
            lookout_medallion.query_silver(
                sql, country=country, params=params, root=self.root
            )
        )

    def countries(self, dataset: str) -> list[str]:
        return lookout_medallion.countries_of(dataset, root=self.root)

    def country_of(self, session_id: str) -> str:
        for country in self.countries("session_sample"):
            held = self._query(
                """
                SELECT count(*) AS samples
                FROM session_sample
                WHERE session_id = $session_id
                """,
                country=country,
                session_id=session_id,
            )
            if held.column("samples")[0].as_py() > 0:
                return country
        raise ValueError(f"no session {session_id} in the store")

    def samples(self, session_id: str, country: str) -> Iterator[Sample]:
        table = self._query(
            """
            SELECT t, lat, lon, alt, acc, speed, heading
            FROM session_sample
            WHERE session_id = $session_id
            ORDER BY t, seq
            """,
            country=country,
            session_id=session_id,
        )
        for row in table.to_pylist():
            yield Sample(
                t=row["t"],
                lat=row["lat"],
                lon=row["lon"],
                alt=row["alt"],
                accuracy_metres=row["acc"],
                speed_mps=row["speed"],
                heading_degrees=row["heading"],
            )

    def crossings(self, country: str) -> list[tuple[int, float, float]]:
        table = self._query(
            """
            SELECT crossing_compact_id AS id, ST_Y(geometry) AS lat, ST_X(geometry) AS lon
            FROM water_crossing
            """,
            country=country,
        )
        return [(row["id"], row["lat"], row["lon"]) for row in table.to_pylist()]

    def sessions(self) -> list[Session]:
        listed = [
            Session(
                session_id=row["session_id"],
                country=country,
                first=row["first"],
                last=row["last"],
                samples=row["samples"],
            )
            for country in self.countries("session_sample")
            for row in self._query(
                """
                SELECT session_id, min(t) AS first, max(t) AS last, count(*) AS samples
                FROM session_sample
                GROUP BY session_id
                """,
                country=country,
            ).to_pylist()
        ]
        return sorted(listed, key=lambda session: session.first, reverse=True)
