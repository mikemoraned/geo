from collections.abc import Iterator
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

import lookout_medallion
import pyarrow as pa


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

    def _query(self, sql: str, **params) -> pa.Table:
        return pa.table(
            lookout_medallion.query_silver(sql, params=params, root=self.root)
        )

    def samples(self, session_id: str) -> Iterator[Sample]:
        table = self._query(
            """
            SELECT t, lat, lon, alt, acc, speed, heading
            FROM session_sample
            WHERE session_id = $session_id
            ORDER BY t, seq
            """,
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

    def crossings(self, country: str | None = None) -> list[tuple[int, float, float]]:
        where = "WHERE country = $country" if country else ""
        table = self._query(
            f"""
            SELECT crossing_compact_id AS id, ST_Y(geometry) AS lat, ST_X(geometry) AS lon
            FROM water_crossing
            {where}
            """,
            **({"country": country} if country else {}),
        )
        return [(row["id"], row["lat"], row["lon"]) for row in table.to_pylist()]

    def sessions(self) -> list[tuple[str, datetime, datetime, int]]:
        table = self._query(
            """
            SELECT session_id, min(t) AS first, max(t) AS last, count(*) AS samples
            FROM session_sample
            GROUP BY session_id
            ORDER BY first DESC
            """
        )
        return [
            (row["session_id"], row["first"], row["last"], row["samples"])
            for row in table.to_pylist()
        ]
