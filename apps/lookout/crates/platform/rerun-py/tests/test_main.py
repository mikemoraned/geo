import socket

import pytest

from runner.main import arguments, listening, viewer_address


def test_listing_and_drawing_are_separate_commands():
    assert arguments(["sessions"]).command == "sessions"
    assert arguments(["replay", "abc"]).command == "replay"


def test_a_viewer_is_the_one_on_this_machine_unless_another_is_named():
    assert arguments(["replay", "abc"]).url is None
    assert arguments(["replay", "abc", "--url", "rerun+http://host:9876/proxy"]).url == (
        "rerun+http://host:9876/proxy"
    )


def test_a_replay_names_the_session_it_replays():
    assert arguments(["replay", "abc"]).session == "abc"


def test_a_replay_without_a_session_is_an_error():
    with pytest.raises(SystemExit):
        arguments(["replay"])


def test_naming_no_command_is_an_error():
    with pytest.raises(SystemExit):
        arguments([])


def test_a_replay_scans_within_five_kilometres_unless_told_otherwise():
    assert arguments(["replay", "abc"]).radius_metres == 5_000.0


def test_a_replay_scans_within_the_radius_it_is_given():
    assert arguments(["replay", "abc", "--radius-metres", "2000"]).radius_metres == 2_000.0


@pytest.mark.parametrize(
    ("radius", "reason"),
    [
        ("0", "positive, finite"),
        ("-2000", "positive, finite"),
        ("inf", "positive, finite"),
        ("nan", "positive, finite"),
        ("far", "not a number of metres"),
    ],
)
def test_a_radius_that_is_not_positive_and_finite_is_refused_saying_why(
    radius, reason, capsys
):
    with pytest.raises(SystemExit):
        arguments(["replay", "abc", f"--radius-metres={radius}"])

    assert reason in capsys.readouterr().err


def test_the_store_to_read_is_the_callers_to_choose_on_either_command(tmp_path):
    assert arguments(["sessions", "--medallion-root", str(tmp_path)]).medallion_root == tmp_path
    assert (
        arguments(["replay", "abc", "--medallion-root", str(tmp_path)]).medallion_root
        == tmp_path
    )


class TestTheViewerToDrawIn:
    def test_a_viewer_on_this_machine_is_where_rerun_listens(self):
        assert viewer_address(None) == ("127.0.0.1", 9876)

    def test_a_named_viewer_is_taken_apart_into_a_host_and_a_port(self):
        assert viewer_address("rerun+http://elsewhere:1234/proxy") == ("elsewhere", 1234)

    def test_a_port_nothing_listens_on_is_not_a_viewer(self):
        with socket.socket() as taken:
            taken.bind(("127.0.0.1", 0))
            free = taken.getsockname()[1]

        assert not listening(("127.0.0.1", free))

    def test_a_port_something_accepts_on_is(self):
        with socket.socket() as accepting:
            accepting.bind(("127.0.0.1", 0))
            accepting.listen()

            assert listening(accepting.getsockname())
