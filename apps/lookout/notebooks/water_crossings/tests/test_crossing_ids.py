"""What the ids are for: that two runs over the same reference data name the same things.

The failure these exist to catch is silent — ids that differ between runs still write, and only
show up later as ground truth and predictions that never match.
"""

import random

import pytest

from crossing_ids import SEPARATOR, crossing_id, crossing_ids, track_ids

# A connector shared by two segments joins them; `c9` is on one segment only.
SEGMENTS = [
    ("seg-b", ["c1", "c2"]),
    ("seg-a", ["c2", "c3"]),
    ("seg-c", ["c3"]),
    ("seg-lone", ["c9"]),
]


class TestNamingATrack:
    def test_segments_sharing_a_connector_are_one_track(self):
        tracks = track_ids(SEGMENTS)

        assert tracks["seg-a"] == tracks["seg-b"] == tracks["seg-c"]
        assert tracks["seg-lone"] != tracks["seg-a"]

    def test_a_track_is_named_by_its_smallest_member(self):
        tracks = track_ids(SEGMENTS)

        assert tracks["seg-b"] == "seg-a"
        assert tracks["seg-lone"] == "seg-lone"

    def test_the_name_does_not_depend_on_the_order_the_segments_were_read(self):
        """The bug a graph library's own component labels have: they number by row order."""
        shuffled = SEGMENTS.copy()
        random.Random(0).shuffle(shuffled)

        assert track_ids(shuffled) == track_ids(SEGMENTS)

    def test_a_segment_with_no_connectors_is_its_own_track(self):
        tracks = track_ids([("seg-a", None), ("seg-b", [])])

        assert tracks == {"seg-a": "seg-a", "seg-b": "seg-b"}

    def test_connectors_may_arrive_as_the_upstream_struct(self):
        """Which is the shape the reference data's own column has."""
        tracks = track_ids(
            [
                ("seg-b", [{"connector_id": "c1"}, {"connector_id": "c2"}]),
                ("seg-a", [{"connector_id": "c2"}]),
            ]
        )

        assert tracks == {"seg-a": "seg-a", "seg-b": "seg-a"}

    def test_a_track_keeps_its_name_when_another_track_appears(self):
        """Re-extracting adds segments elsewhere; that must not rename what is unchanged."""
        before = track_ids(SEGMENTS)

        after = track_ids([*SEGMENTS, ("seg-z", ["c8"]), ("seg-y", ["c8"])])

        assert after["seg-a"] == before["seg-a"]
        assert after["seg-z"] == "seg-y"


class TestNamingACrossing:
    def test_the_same_water_and_track_name_the_same_crossing(self):
        assert crossing_id("water-1", "seg-a") == crossing_id("water-1", "seg-a")

    def test_a_different_water_or_track_is_a_different_crossing(self):
        assert crossing_id("water-1", "seg-a") != crossing_id("water-2", "seg-a")
        assert crossing_id("water-1", "seg-a") != crossing_id("water-1", "seg-b")

    def test_an_id_does_not_depend_on_the_part_that_represents_the_crossing(self):
        """Retuning the collapse moves the representative part and merges different parts.

        The crossing itself is unchanged by that, so its id has to be — which holds because
        nothing about a part reaches the id.
        """
        assert crossing_id("water-1", "seg-a") == crossing_id("water-1", "seg-a")
        assert crossing_ids(["water-1", "water-1"], ["seg-a", "seg-a"]) == [
            crossing_id("water-1", "seg-a")
        ] * 2

    def test_ids_are_paired_up_positionally(self):
        assert crossing_ids(["w1", "w2"], ["t1", "t2"]) == [
            crossing_id("w1", "t1"),
            crossing_id("w2", "t2"),
        ]

    @pytest.mark.parametrize("reserved", ["/", " ", "="])
    def test_an_id_could_name_a_partition(self, reserved):
        """The store's rule for a value that names a directory, since an id is a candidate
        key for one wherever a reader lays a dataset out by it."""
        assert reserved not in SEPARATOR
        assert reserved not in crossing_id("08f2a5c1", "08f2a5c2")
