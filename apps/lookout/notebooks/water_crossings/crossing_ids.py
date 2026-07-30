"""Naming the things the crossings pipeline finds, so two runs agree on what they found.

A crossing is where a stretch of physical track meets a body of water. Both parts of that need
a name that follows from the data rather than from the run:

  * a **track** is a connected component of rail segments — segments joined end to end by a
    shared connector. Labelling components with the numbers a graph library hands back names
    them by row order, so the same track is component 7 in one run and 12 in the next. Here a
    component is named by the lexically smallest segment in it, which follows from its members.
  * a **crossing** is then named by the water and the track that meet, and by nothing else. In
    particular not by the part that represents it: the collapse picks a representative by
    overlap length and merges by distance, so a crossing's representative moves when that
    tuning changes while the crossing itself does not.

Ground truth recorded by one run and a prediction made by another therefore refer to the same
crossing, and a rerun over the same extract rewrites rows rather than adding them.
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping

import numpy as np
import scipy.sparse as sparse
import scipy.sparse.csgraph as csgraph

# Separates the parts of a crossing id. A composite rather than a hash of the two: both parts
# are already columns of the row, so an opaque id would hide what it is made of while saying
# nothing more, and a mismatch between a prediction and the ground truth is read by eye.
SEPARATOR = ":"


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

    labels = _components(len(ids), _shared_connectors(segments, index))

    canonical: dict[int, str] = {}
    for segment_id, label in zip(ids, labels):
        canonical[label] = min(canonical.get(label, segment_id), segment_id)
    return {
        segment_id: canonical[label] for segment_id, label in zip(ids, labels)
    }


def crossing_id(water_id: str, track_id: str) -> str:
    """The id of the crossing where `track_id` meets `water_id`."""
    return f"{water_id}{SEPARATOR}{track_id}"


def crossing_ids(water_ids: Iterable[str], tracks: Iterable[str]) -> list[str]:
    """The ids of the crossings named by each `(water_id, track_id)` pair, pairwise."""
    return [
        crossing_id(water, track) for water, track in zip(water_ids, tracks)
    ]


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


def _components(nodes: int, edges: np.ndarray) -> np.ndarray:
    """Connected-component label per node of an undirected `nodes`-node graph.

    The labels are the graph library's own and depend on row order — they are used only to
    group, never to name.
    """
    graph = sparse.coo_matrix(
        (np.ones(len(edges)), (edges[:, 0], edges[:, 1])),
        shape=(nodes, nodes),
    )
    return csgraph.connected_components(graph, directed=False)[1]
