https://docs.astral.sh/uv/guides/integration/jupyter/

https://boundingbox.klokantech.com

TODOs:
* [x] setup jupyter env that can read overturemaps data from local copy using duckdb
    * follow https://docs.overturemaps.org/examples/pandas/ / https://notebooksharing.space/view/1d0d72d24ed82d22a8101377ca811ab7365b6a67dac98f3add034719c44b537f#displayOptions=
    `uv run jupyter lab`
* [x] use lonboard
* [x] get data directly from overturemaps via overturemaps.py as may be fast enough for what I need
* [x] get data about tube stations in London
    * [x] load segments in a sub-area of London
        * https://boundingbox.klokantech.com
    * [x] load connectors as well as segments
    * [x] find which of these connectors correspond to tube stations
    * [x] find all tube stations in London
        * I've *sort-of* done this in that I have found all connectors that lay on a segment that is a subway
* [ ] get buildings in the area of interest and find what types there are (from OvertureMaps)
    * [x] get buildings in an area
    * [ ] ...
* [ ] find examples of geoai datasets / models I could use