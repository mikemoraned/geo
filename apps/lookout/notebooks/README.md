# Notebooks

This README covers advice for writing lookout's notebooks. It is for whoever writes or changes one.
[The python extensions](../README.md#the-python-extensions) covers taking an extension into a
notebook.

## Drawing A5 cells

Two precautions apply when drawing [A5](https://a5geo.org/) cells on a map:

- **[lonboard](https://developmentseed.org/lonboard/) 0.17.0's `A5Layer` draws
  [`pya5`](https://pypi.org/project/pya5/1.0.0/) 1.0.0's cells in the wrong place.** A polygon from
  `pya5`'s `cell_to_boundary`, drawn in a `PolygonLayer`, lands in the right place.
- **`cell_to_boundary` returns a cell's five corners alone by default.** An A5 cell's edges curve in
  longitude and latitude, so a point near an edge can fall outside the drawn outline. With
  `{"segments": 50}`, each edge takes 50 segments, and every point inside the cell falls inside its
  outline.
