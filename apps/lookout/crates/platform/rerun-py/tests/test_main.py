"""What the command does with its arguments.

Each command says what it does in its own name, so nothing is decided by an argument being
absent: `sessions` lists, `replay` draws in a viewer, `record` writes a file, and none of the
three becomes another.
"""

import socket

import pytest

from runner.main import arguments, listening, viewer_address


def test_listing_drawing_and_writing_are_separate_commands():
    assert arguments(["sessions"]).command == "sessions"
    assert arguments(["replay", "abc"]).command == "replay"
    assert arguments(["record", "abc"]).command == "record"


def test_a_viewer_is_the_one_on_this_machine_unless_another_is_named():
    assert arguments(["replay", "abc"]).url is None
    assert arguments(["replay", "abc", "--url", "rerun+http://host:9876/proxy"]).url == (
        "rerun+http://host:9876/proxy"
    )


def test_only_a_recording_written_to_a_file_names_one():
    assert arguments(["record", "abc"]).output is None
    assert not hasattr(arguments(["replay", "abc"]), "output")


def test_a_replay_names_the_session_it_replays():
    assert arguments(["replay", "abc"]).session == "abc"


def test_a_replay_without_a_session_is_an_error():
    with pytest.raises(SystemExit):
        arguments(["replay"])

    with pytest.raises(SystemExit):
        arguments(["record"])


def test_naming_no_command_is_an_error():
    with pytest.raises(SystemExit):
        arguments([])


def test_a_replay_scans_germany_within_five_kilometres_unless_told_otherwise():
    args = arguments(["replay", "abc"])

    assert args.country == "DE"
    assert args.radius_metres == 5_000.0


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
