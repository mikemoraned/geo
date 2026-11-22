# test-predictor

Sentences in the shape the M5 board's GNSS receiver emits them, for tests. Tests that drive a
parser or a core with sentences take this crate as a dev-dependency.

The constants are captures with the position replaced. `Fix` builds a sentence for a fix at a
chosen time and place, and `captured` reads a constant as a `Sentence`.
