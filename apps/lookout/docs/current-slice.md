# Current Slice: getting a second source of position data from Motis

### Target

Whilst I am travelling on a train I'd like to get a secondary (non-GPS) source of data by periodically polling a 
local [Motis](https://github.com/motis-project/motis/) instance running with data for Germany. 

#### Info

I have a local motis server installation (see tools/motis-server/Justfile) running with data for Germany. It is listening on http://localhost:8080.

### Straw Man Architecture

The idea would be to do something like the following in a continuous loop:
1. Poll the redis queue for recently logged gps positions, covering the past N minutes (ignore anything old)
2. Maintain a local set which contains all positions seen over past 30 mins
3. Building a bounding-box which covers the area of these GPS positions, expanded with a buffer; let's say double the size
4. Query motis for this bounding box to find all train positions in this region
5. Log this data to a local sqlite `motis` db, with duplication allowed

The intent is then to take this raw data in the db, and ingest it alongside the existing gps data in the `lookout` db to produce a visualisation of train positions over the same time period as the gps traces being visualised.
