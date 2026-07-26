# Next Slices

## Slice: minimal predictor and evaluation framework 

### Target

We should now have enough to put together a first minimal predictor that is based solely on crow-flies distance, and also to evaluate how good it is.

## Straw man

The essential idea here is to use our collected traces along with known water crossings to both act as source data and as measurement.

### Sessionisation

So, we first find all gps readings from real traces and "sessionise" them. This effectively boils down to breaking the data from the same device into sessions whenever:
1. There is an explicit `StartSession` message
2. There is a gap of N minutes between successive readings (N = 10 minutes probably good enough)

### Water crossings per session

Then, we look at any gps readings in each session that come within M metres of a known water crossing. This is where it is probably a good idea to first normalise a session to include a version of the path that has a CRS in metres (a projected CRS). Same goes for the water crossings dataset. For simplicity, since we are covering Germany, it'd be enough to use a single CRS for now.

Once we have some gps readings for each water crossing for each session, we minimise this to just a single example for each water crossing per trace, using the closest match. This should give us a set of water crossings per session. We treat this as our ground truth.

### Simple crow-flies predictor

We then implement a simple predictor with a prediction cycle which functions something like:
1. Receive latest GPS reading
2. Find all water crossings within D distance (in metres); remember this for later
3. If we have a previous set of water crossings:
    * find overlap between sets, and compare distances for each pair of old and new, and work out distance delta (delta = new - old)
    * for those where delta is negative (we've gotten closer) calculate velocity
    * emit prediction of wall-clock time we will pass each water crossing based on current distance to water crossing and current velocity towards it

In this simple predictor we are not taking advantage of any speed or heading information in the GPS readings. That will be sensible to include later, but for now we can keep it simple. Later on we'll likely want to include any additional information we have in a sensor-fusion approach but for now we keep it simple.

### Evaluation framework

We can think of a predictor as attempting to fill in, at each prediction cycle, a 2D space where the y-axis is all the possible water crossings and the x-axis is the time at which the water will be crossed in this session. 

We can run this for each gps reading in each session (each prediction cycle) and then assess as follows:
* precision = for each water crossing, whenever we predicted that we would cross at time T_P, what was the actual T_A, and was it within some tolerance e.g. 30 seconds. Similarly, was it within some distance. Count each of these as a boolean yes/no
* recall = for each water crossing that was really passed in a session, did we make a prediction for it?

This measurement framework and predictor can both likely be improved, but it's probably enough to start with.

## Refactor to Medallion Architecture

I think at this point we need to cleanly separate our bits of data processing and storage into a [medallion architecture](https://motherduck.com/glossary/medallion-architecture/). In this context this means something like:
* bronze:
    * raw gps and accel sensor readings, recorded live in redis and extracted via `recorder`
    * motis train samples, recorded via `motis_poll`
    * point in time extracts from OvertureMaps restricted to our needs e.g. rail/water for Germany
* silver:
    * gps readings sessionised and normalised into standard geometries
    * derived water crossings, represented as an enriched OvertureMaps segments and connector dataset extended/restricted to only what we need
* gold:
    * results of runs evaluating particular predictor versions against silver datasets
    * preparations of data for live usage externally

We also want to start standardising on representions i.e. where possible we should use parquet, but with different biases in each section:
* bronze:
    * parquet optimised for quick append of new data; we generally never delete anything from here, and we want to make appends quick and save. The structures should be biased towards sample structure e.g. each unique poll by `motis_poll` should get a timestamp which records when the poll happened and this should be part of the folder structure.
    * *if we receive it from somewhere* we allow storage here in compact geo formats like [polyline](https://developers.google.com/maps/documentation/utilities/polylinealgorithm) as that's the sort of formats live services use for capturing paths. note that this is not our preferred format for everything in bronze as it doesn't allow representation of everything we care about, but if we receive it from some third-party service then we should store it.
        * if we are generating our own geo formats then we should favour using the same formats as in silver
    * we store extracts from OvertureMaps here largely in the native format they use, but add metadata like when what version of overturemaps was used (in case not already present). this extract may come from a bounding-box restriction we applied so we also add that as metadata
        * the metadata can be stored in a separate table I own, for example containing data of extract, unique extract id, and bounding box. the overture maps table may then only need to be enriched with the extract id as an additional column.
* silver:
    * here parquet is optimised for fast and scalable lookup and processing. this means embedding whatever metadata possible (like bounding boxes) to make queries faster
    * we should use [GeoParquet](https://geoparquet.org) and ensure everything represents geographic concepts in the same way
        * there is also GEOMETRY/GEOGRAPHY [geospatial types](https://parquet.apache.org/docs/file-format/types/geospatial/) but these aren't well-supported by many apps / libraries right now
    * when we are extending/subsetting OvertureMaps data and storing here, we should always follow their schemas where possible, even for our own extensions to the data. however, additional, even when we are creating our own data from scratch, we should still follow their schemas as they are likely suitable for what we are doing as well
    * when storing paths or other geo entities, we should *always* have a normalised clean lat/lon representation in a global CRS
        * optionally, we can also eagerly pre-calculate a column in a project CRS which is most appropriate for the entity. So, for example, for segments in 
* gold:
    * this is where we may produce specialised output formats, like [PMTiles](https://docs.protomaps.com/pmtiles/) / [protomaps](https://protomaps.com/about), intended to be used by live systems. This is also again where things like polylines are allowed/encouraged.

The root where this data is stored is ~/Data/geo/lookout/medallion. If this becomes took big, then we'll move to store it on /Volumes/PRO-G40/Data/geo/lookout/medallion (my external drive). Data should be stored in Hive format.

One intent here is to standardis to allow multiple writer/readers, which are different engines, as appropriate i.e. Duckdb, SedonaDB, georust.

### Tasks

...

## Sessionisation

### Tasks 

...

## Water crossings per session

We probably need to here productionise the pipeline we prototyped in apps/lookout/notebooks/water_crossings/v7.py. However, it's ok to keep it as a notebook, or chain of notebooks, for now.

### Tasks 

...

## Simple crow-flies predictor

This should probably be written in Rust as this will be the beginnings of what we later embed in a live system. So, we may need to put some wrappers around it to make it easy to call from Python as part of the eval framework (see below).

### Tasks 

...

## Evaluation framework

This should be written in marimo notebooks and try to re-use as much typical evaluation libraries as possible. So, once we've defined our precision/recall definitions I'd like to plug those into standard well-supported python libraries which allows us to define things like F1-score on top.

### Tasks 

...

## Slice: Enrich and use relative direction of POI

### Target

Enrich the water crossings dataset with an angle relative to the train line and travel direction. This allows a recommendation to be given about which direction to look relative to the train seat.

## Slice: Adding POI's from images taken

### Idea

Assuming we have an iOS App, and it is running whilst people are taking pictures, we can support adding POI's by correlating what the position of the person was and on what line when they took the picture. We can also access the compass sensor to get the direction of the phone at the time. This allows us to establish an angle to the POI relative to the train and so remember what direction you'd need to be facing to be able to see it again.

An onboard model could perhaps be used to do rough interpretation of kind of POI e.g. is it a building or a river or what.

We probably don't want to go down the lines of storing the image, but perhaps there is some on-device or privacy-preserving way to to identify exactly what the POI is based on the image.