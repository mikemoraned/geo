"""Replay a recorded session through the predictor, into a rerun viewer or a file.

Three commands, each doing one thing:

    just sessions               the sessions the store holds
    just replay <session-id>    that session, drawn in a viewer already running
    just record <session-id>    the same, written to a .rrd to keep

`replay` needs a viewer listening, which `rerun` on its own starts. `record` answers the
other need: a recording that can be opened later, or held beside another run of the same
session to compare.
"""

import argparse
import socket
from pathlib import Path
from urllib.parse import urlparse

import lookout_medallion
import rerun as rr
from lookout_predictor import DEFAULT_RADIUS_METRES, CrowFlies

from .blueprint import blueprint
from .log import draw
from .replay import replay
from .store import Store

APPLICATION = "lookout-predictor30"

# Where a viewer started as `rerun` listens.
DEFAULT_VIEWER = "rerun+http://127.0.0.1:9876/proxy"


def viewer_address(url: str | None) -> tuple[str, int]:
    """The host and port `url` names, or those a viewer on this machine listens on."""
    viewer = urlparse(url or DEFAULT_VIEWER)
    return viewer.hostname or "127.0.0.1", viewer.port or 9876


def listening(address: tuple[str, int]) -> bool:
    """Whether anything accepts a connection at `address`.

    Asked before drawing, because a stream with nowhere to send drops what it is given and
    says nothing: without this, a replay into no viewer is silent and looks like success.
    """
    try:
        with socket.create_connection(address, timeout=1):
            return True
    except OSError:
        return False


def default_output() -> Path:
    """Beside the store, so a recording lands in the same place wherever it is run from."""
    return Path(lookout_medallion.default_root()).parent / "predictor.rrd"


def arguments(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)

    store = argparse.ArgumentParser(add_help=False)
    store.add_argument("--medallion-root", type=Path, help="the store to read")

    session = argparse.ArgumentParser(add_help=False)
    session.add_argument("session", help="the session to replay")
    session.add_argument(
        "--country",
        default="DE",
        help="restrict the crossings scanned against to one country's",
    )
    session.add_argument(
        "--radius-metres",
        type=float,
        default=DEFAULT_RADIUS_METRES,
        help="how far ahead to predict",
    )

    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("sessions", parents=[store], help="list the sessions the store holds")

    live = commands.add_parser(
        "replay", parents=[store, session], help="draw a session in a running viewer"
    )
    live.add_argument("--url", help="the viewer to draw in, if not the one on this machine")

    saved = commands.add_parser(
        "record", parents=[store, session], help="write a session to a .rrd"
    )
    saved.add_argument("--output", type=Path, help="where to write the recording")

    return parser.parse_args(argv)


def sessions(store: Store) -> None:
    for session, first, last, samples in store.sessions():
        print(f"{session}  {first:%Y-%m-%d %H:%M}..{last:%H:%M}  {samples} samples")


def replayed(store: Store, args: argparse.Namespace) -> rr.RecordingStream:
    """A recording of `args.session` replayed, with its sink already chosen.

    The sink is set before anything is drawn, since a stream sends as it is logged rather
    than keeping what was logged before it had somewhere to send it.
    """
    recording = rr.RecordingStream(APPLICATION)

    if args.command == "replay":
        address = viewer_address(args.url)
        if not listening(address):
            host, port = address
            raise SystemExit(
                f"no rerun viewer is listening on {host}:{port}; start one with `rerun`"
            )
        recording.connect_grpc(args.url, default_blueprint=blueprint())
    else:
        output = args.output or default_output()
        output.parent.mkdir(parents=True, exist_ok=True)
        recording.save(str(output), default_blueprint=blueprint())
        print(f"writing {output}")

    crossings = store.crossings(country=args.country)
    passings = store.passings(args.session)
    predictor = CrowFlies(crossings, radius_metres=args.radius_metres)
    steps = replay(predictor, store.samples(args.session))

    draw(recording, steps, crossings, passings)
    return recording


def main(argv: list[str] | None = None) -> None:
    args = arguments(argv)
    store = Store(args.medallion_root)

    try:
        if args.command == "sessions":
            sessions(store)
        else:
            # Blocking, so the process outlives what it is still sending: a viewer that
            # receives half a session is worse than one that waits for all of it.
            replayed(store, args).flush()
    # A store whose sessions or crossings have never been derived is an ordinary state to
    # find it in, and says so in one line rather than as a stack.
    except ValueError as absent:
        raise SystemExit(str(absent)) from absent


if __name__ == "__main__":
    main()
