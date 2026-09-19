"""Reading a session's samples, and the crossings to scan them against.

Queried through `lookout_medallion.query_silver`, which registers a dataset under its own
name: which files hold it, and what CRS its geometry is in, are the store's to know rather
than this reader's. So no path is spelled out here, and a crossing's position arrives as a
pair of coordinates rather than as WKB to decode.
"""

from collections.abc import Iterator
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

import lookout_medallion
import pyarrow as pa


@dataclass(frozen=True)
class Sample:
    """One fix of a session, as `session_sample` holds it.

    Every field past the position is what the recording device happened to report, so a
    field it left out is `None` here and unknown to the predictor.
    """

    t: datetime
    lat: float
    lon: float
    alt: float | None
    accuracy_metres: float | None
    speed_mps: float | None
    heading_degrees: float | None


class Store:
    """One medallion store, queried with SQL.

    `root` names it, defaulting to the store in the repo the caller is working in.

    A dataset that has never been derived raises a `ValueError` naming it, rather than
    reading as a dataset with no rows: a store without crossings cannot be replayed against,
    and saying so is more use than an empty map.
    """

    def __init__(self, root: Path | None = None) -> None:
        self.root = None if root is None else str(root)

    def _query(self, sql: str, **params) -> pa.Table:
        return pa.table(
            lookout_medallion.query_silver(sql, params=params, root=self.root)
        )

    def samples(self, session_id: str) -> Iterator[Sample]:
        """The samples of `session_id`, oldest first.

        Ordered by `t` and then by `seq`, since two samples share an instant where a device
        reports faster than its clock resolves.
        """
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
        """The crossings to scan against, each `(id, latitude, longitude)`.

        Named by `crossing_short_id`, which is the name a prediction comes back under.
        `country` restricts to one partition.
        """
        where = "WHERE country = $country" if country else ""
        table = self._query(
            f"""
            SELECT crossing_short_id AS id, ST_Y(geometry) AS lat, ST_X(geometry) AS lon
            FROM water_crossing
            {where}
            """,
            **({"country": country} if country else {}),
        )
        return [(row["id"], row["lat"], row["lon"]) for row in table.to_pylist()]

    def sessions(self) -> list[tuple[str, datetime, datetime, int]]:
        """Every session that has samples, as `(id, first, last, samples)`, newest first."""
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
