"""Replay a recorded session through the predictor, into a running rerun viewer.

    just sessions               the sessions the store holds
    just replay <session-id>    that session, drawn in a viewer already running

`replay` needs a viewer listening, which `rerun` on its own starts.
"""


import argparse
import socket
from pathlib import Path
from urllib.parse import urlparse

import rerun as rr
from lookout_predictor import DEFAULT_RADIUS_METRES, CrowFlies

from .blueprint import blueprint
from .log import draw
from .replay import replay
from .store import Store

APPLICATION = "lookout-predictor30"

DEFAULT_VIEWER = "rerun+http://127.0.0.1:9876/proxy"


def viewer_address(url: str | None) -> tuple[str, int]:
    viewer = urlparse(url or DEFAULT_VIEWER)
    return viewer.hostname or "127.0.0.1", viewer.port or 9876


def listening(address: tuple[str, int]) -> bool:
    try:
        with socket.create_connection(address, timeout=1):
            return True
    except OSError:
        return False


def arguments(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)

    store = argparse.ArgumentParser(add_help=False)
    store.add_argument("--medallion-root", type=Path, help="the store to read")

    session = argparse.ArgumentParser(add_help=False)
    session.add_argument("session", help="the session to replay")
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

    return parser.parse_args(argv)


def sessions(store: Store) -> None:
    for listed in store.sessions():
        print(
            f"{listed.session_id}  {listed.country}  "
            f"{listed.first:%Y-%m-%d %H:%M}..{listed.last:%H:%M}  {listed.samples} samples"
        )


def replayed(store: Store, args: argparse.Namespace) -> rr.RecordingStream:
    recording = rr.RecordingStream(APPLICATION)

    address = viewer_address(args.url)
    if not listening(address):
        host, port = address
        raise SystemExit(
            f"no rerun viewer is listening on {host}:{port}; start one with `rerun`"
        )
    recording.connect_grpc(args.url, default_blueprint=blueprint())

    country = store.country_of(args.session)
    crossings = store.crossings(country)
    predictor = CrowFlies(crossings, radius_metres=args.radius_metres)
    steps = replay(predictor, store.samples(args.session, country))

    draw(recording, steps, crossings)
    return recording


def main(argv: list[str] | None = None) -> None:
    args = arguments(argv)
    store = Store(args.medallion_root)

    try:
        if args.command == "sessions":
            sessions(store)
        else:
            replayed(store, args).flush()
    except ValueError as absent:
        raise SystemExit(str(absent)) from absent


if __name__ == "__main__":
    main()
