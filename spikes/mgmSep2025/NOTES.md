https://docs.astral.sh/uv/guides/integration/jupyter/

https://boundingbox.klokantech.com

TODOs:
* [x] setup jupyter env that can read overturemaps data from local copy using duckdb
    * follow https://docs.overturemaps.org/examples/pandas/ / https://notebooksharing.space/view/1d0d72d24ed82d22a8101377ca811ab7365b6a67dac98f3add034719c44b537f#displayOptions=
    `uv run jupyter lab`
* [x] use lonboard
* [x] get data directly from overturemaps via overturemaps.py as may be fast enough for what I need
* [ ] get data about tube stations in London
    * [x] load segments in a sub-area of London
        * https://boundingbox.klokantech.com
    * [ ] load connectors as well as segments
    * [ ] find which of these connectors correspond to tube stations
    * [ ] find all tube stations in London