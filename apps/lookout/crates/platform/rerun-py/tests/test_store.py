import datetime

import pytest

from runner.store import Session, Store

from conftest import (
    JUST_BEFORE_MIDNIGHT,
    COMES_INSIDE_THE_RADIUS,
    COUNTRY,
    ELSEWHERE,
    ELSEWHERE_SESSION,
    FAR,
    LON,
    NEAR,
    SESSION,
    STAYS_OUTSIDE_IT,
)


def test_a_sessions_samples_come_back_oldest_first_across_its_partitions(store):
    samples = list(Store(store).samples(SESSION, COUNTRY))

    assert [sample.t for sample in samples] == [
        JUST_BEFORE_MIDNIGHT + datetime.timedelta(minutes=minute) for minute in range(4)
    ]
    assert samples[0].lat == 50.0
    assert samples[-1].lat == 50.03
    assert samples[0].lon == LON


def test_a_field_the_device_left_out_reads_as_unknown(store):
    first, second = list(Store(store).samples(SESSION, COUNTRY))[:2]

    assert first.speed_mps is None
    assert first.alt is None
    assert second.speed_mps == 18.5
    assert second.alt == 12.5
    assert first.accuracy_metres == 4.8
    assert first.heading_degrees == 0.0


def test_a_session_the_store_has_never_seen_has_no_samples(store):
    assert list(Store(store).samples("no-such-session", COUNTRY)) == []


def test_the_crossings_come_back_named_by_the_id_a_device_holds(store):
    crossings = Store(store).crossings(COUNTRY)

    assert sorted(crossings) == sorted(
        [(NEAR, COMES_INSIDE_THE_RADIUS, LON), (FAR, STAYS_OUTSIDE_IT, LON)]
    )


def test_the_crossings_come_from_the_country_the_read_names(store):
    assert len(Store(store).crossings(COUNTRY)) == 2

    with pytest.raises(ValueError, match="ZZ"):
        Store(store).crossings("ZZ")


def test_a_dataset_that_has_never_been_derived_says_so(empty_store):
    empty = Store(empty_store)

    with pytest.raises(ValueError, match="water_crossing"):
        empty.crossings(COUNTRY)

    with pytest.raises(ValueError, match="session_sample"):
        list(empty.samples(SESSION, COUNTRY))


def test_a_session_is_listed_with_the_span_and_count_that_name_it(store):
    listed = Store(store).sessions()

    last = JUST_BEFORE_MIDNIGHT + datetime.timedelta(minutes=3)

    assert listed[0] == Session(SESSION, COUNTRY, JUST_BEFORE_MIDNIGHT, last, 4)


def test_every_country_contributes_its_sessions_newest_first(store):
    listed = Store(store).sessions()

    assert [(one.session_id, one.country) for one in listed] == [
        (SESSION, COUNTRY),
        (ELSEWHERE_SESSION, ELSEWHERE),
    ]


def test_a_session_is_found_in_the_country_it_was_recorded_in(store):
    reader = Store(store)

    assert reader.country_of(SESSION) == COUNTRY
    assert reader.country_of(ELSEWHERE_SESSION) == ELSEWHERE


def test_a_session_no_country_holds_is_reported(store):
    with pytest.raises(ValueError, match="no-such-session"):
        Store(store).country_of("no-such-session")
