"""The rerun runner: a recorded session replayed through the predictor.

`store` reads a session's samples and the crossings to scan them against, and `replay` feeds
those samples through the predictor in `t` order.
"""
