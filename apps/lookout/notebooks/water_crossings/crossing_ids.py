"""Naming the things the crossings pipeline finds, so two runs agree on what they found.

A crossing is where a stretch of physical track meets a body of water. Both parts of that need
a name that follows from the data rather than from the run:

  * a **track** is a connected component of rail segments — segments joined end to end by a
    shared connector. Labelling components with the numbers a graph library hands back names
    them by row order, so the same track is component 7 in one run and 12 in the next. Here a
    component is named by the lexically smallest segment in it, which follows from its members.
  * a **crossing** is then named by the water, the track, and where along the track the two
    meet. The place is part of the name because one track crosses one body of water more than
    once: a line following a valley crosses the river it runs beside repeatedly, and those are
    separate sightings, not one. Naming a crossing by the water and the track alone would give
    them all the same name — measured on the recorded extract, 393 pairs hold between 2 and 13
    crossings, and the largest of those lie on a single rail segment, so the segment does not
    separate them either.

Ground truth recorded by one run and a prediction made by another therefore refer to the same
crossing, and a rerun over the same extract rewrites rows rather than adding them. What an id
does *not* survive is a change to the collapse tuning that moves which part represents a
crossing — the position in the name is that part's. A crossing whose merged parts are unchanged
keeps its id, which is every crossing when the tuning is unchanged, and most of them when it is.
"""

from __future__ import annotations

import hashlib
from collections.abc import Iterable, Mapping

import numpy as np
import scipy.sparse as sparse
import scipy.sparse.csgraph as csgraph

# Separates the parts of a crossing id, and marks the position within the last one. A composite
# rather than a hash: every part is already a column of the row, so an opaque id would hide what
# it is made of while saying nothing more, and a prediction that fails to match its ground truth
# is read by eye.
SEPARATOR = ":"
POSITION = "@"

# Decimal places the position is written to. A segment is parameterised from 0 at its start to 1
# at its end, so this is a centimetre on a 10 km segment — far finer than the tens of metres that
# separate two crossings of the same water, and fixed width, so ids compare as strings.
POSITION_PLACES = 6


def track_ids(segments) -> dict[str, str]:
    """Map each rail segment id to the canonical id of the track it belongs to.

    `segments` is an iterable of `(segment_id, connectors)`, where `connectors` is that
    segment's connectors — each either a connector id or the upstream struct carrying one, and
    `None` where the segment has none. Two segments sharing a connector are the same track.

    The canonical id of a track is the lexically smallest segment id in it, so the name follows
    from the component's members and not from the order they were read in.
    """
    segments = list(segments)
    ids = [segment_id for segment_id, _ in segments]
    index = {segment_id: row for row, segment_id in enumerate(ids)}

    labels = component_labels(len(ids), _shared_connectors(segments, index))

    canonical: dict[int, str] = {}
    for segment_id, label in zip(ids, labels):
        canonical[label] = min(canonical.get(label, segment_id), segment_id)
    return {
        segment_id: canonical[label] for segment_id, label in zip(ids, labels)
    }


def crossing_id(
    water_id: str, track_id: str, rail_id: str, frac: float
) -> str:
    """The id of the crossing where `track_id` meets `water_id` at `frac` along `rail_id`.

    `frac` is the position of the crossing along that one segment, from 0 at its start to 1 at
    its end — the `ST_LineLocatePoint` of the representative part. It is what separates the
    several crossings of one water body by one track, and `rail_id` is what gives it a meaning:
    each segment is parameterised on its own, so the same fraction of two segments of one track
    are different places.
    """
    return (
        f"{water_id}{SEPARATOR}{track_id}{SEPARATOR}"
        f"{rail_id}{POSITION}{frac:.{POSITION_PLACES}f}"
    )


def crossing_ids(
    water_ids: Iterable[str],
    tracks: Iterable[str],
    rails: Iterable[str],
    fracs: Iterable[float],
) -> list[str]:
    """The ids of the crossings named by each `(water, track, rail, frac)`, pairwise."""
    return [
        crossing_id(water, track, rail, frac)
        for water, track, rail, frac in zip(water_ids, tracks, rails, fracs)
    ]


# Bytes of the digest a short id is taken from, and the order they are read in. Four bytes is
# what a device has room for beside each coordinate, and little-endian is what it casts them as.
SHORT_ID_BYTES = 4
SHORT_ID_ORDER = "little"


def short_id(crossing_id: str) -> int:
    """The four-byte name of the crossing `crossing_id` names, as an unsigned integer.

    A device holds a crossing as a coordinate and a name, and has no room for the composite id
    above — so it carries this instead. It is a hash of that id rather than a re-derivation from
    the parts behind it, so the two cannot come to disagree about what one crossing is, and a
    prediction made under this name can be looked up by the id it was taken from.

    Four bytes is few enough that two crossings can land on one name by chance; that is the
    store's to refuse when the dataset is written, not this function's to avoid.
    """
    digest = hashlib.md5(crossing_id.encode()).digest()
    return int.from_bytes(digest[:SHORT_ID_BYTES], SHORT_ID_ORDER)


def short_ids(ids: Iterable[str]) -> list[int]:
    """The short name of each of `ids`, pairwise."""
    return [short_id(crossing_id) for crossing_id in ids]


def _shared_connectors(segments, index) -> np.ndarray:
    """Edges between segments that share a connector, as `(row, row)` pairs.

    One edge per pair of segments meeting at the same connector is enough to connect them: the
    components are the same whether or not every such pair is listed.
    """
    seen: dict[str, int] = {}
    edges: list[tuple[int, int]] = []
    for segment_id, connectors in segments:
        for connector in connectors or []:
            connector_id = (
                connector["connector_id"]
                if isinstance(connector, Mapping)
                else connector
            )
            if connector_id in seen:
                edges.append((seen[connector_id], index[segment_id]))
            else:
                seen[connector_id] = index[segment_id]
    return np.asarray(edges, dtype=int).reshape(-1, 2)


def component_labels(nodes: int, edges: np.ndarray) -> np.ndarray:
    """Connected-component label per node of an undirected `nodes`-node graph.

    The labels are the graph library's own and depend on the order the nodes were read in, so
    they are only ever used to **group** — never to name a group. Anything that names one
    derives the name from the group's members, as `track_ids` does.
    """
    graph = sparse.coo_matrix(
        (np.ones(len(edges)), (edges[:, 0], edges[:, 1])),
        shape=(nodes, nodes),
    )
    return csgraph.connected_components(graph, directed=False)[1]
