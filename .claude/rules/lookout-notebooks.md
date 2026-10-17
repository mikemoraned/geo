---
paths:
  - "apps/lookout/**/notebooks/**"
---

# Lookout Notebooks

A notebook is built with the user in a live marimo session, one step at a time, never drafted
alone in a file. The user starts the session. Each step adds or changes one cell, then stops for
review before the next. Querying the store through the medallion binding, for example from the
session's scratchpad, to learn what the data holds before proposing a step is permitted.

A notebook is written so the user can check it for correctness, in two ways:

- **Each query step in its own cell, with a preview.** A long query is split into its named
  steps. Each cell adds one step to the steps before it and shows a small result, such as a count,
  a before-and-after, or a check that a join key is unique. A long chain of steps in one cell
  can't be checked one step at a time.
- **Geographic work shown on a map.** Wherever a join or a measure involves shapes, examples are
  drawn: the whole dataset on a map, chosen cases such as the smallest, median, and largest, and a
  table whose selected rows draw on a map. A wrong shape is easier to spot drawn than as a number.

Datasets load through the medallion library's Python binding, in a cell and while exploring alike,
never through duckdb.
Joins stay in SQL, run by the binding on Sedona and DataFusion, and results stay in Arrow until a
step needs pandas or geopandas.
