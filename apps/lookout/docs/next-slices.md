# Next Slices

## Slice: minimal predictor and evaluation framework 

### Target

If other slices are done, we should have enough to put together a first minimal predictor that is based solely on crow-flies distance, and also to evaluate how good it is.

## Straw man

The essential idea here is to use our collected traces along with known water crossings to both act as source data and as measurement.

So, we first find all gps readings from real traces and "sessionise" them. This effectively boils down to breaking the data from the same device into sessions whenever:
1. There is an explicit `StartSession` message
2. There is a gap of N minutes between successive readings (N = 10 minutes probably good enough)

Then, we look at any gps readings in each session that come within M metres of a known water crossing. This is where it is probably a good idea to first normalise a session to include a version of the path that has a CRS in metres (a projected CRS). Same goes for the water crossings dataset. For simplicity, since we are covering Germany, ideally it'd be good to use a single CRS for now.

Once we have some gps readings for each water crossing for each session, we minimise this to just a single example for each water crossing per trace, using the closest match. This should give us a set of water crossings per session. We treat this as our ground truth.

We then implement a simple predictor which functions something like:
1. Receive latest GPS reading
2. Find all water crossings within D distance (in metres); remember this for later
3. If we have a previous set of water crossings:
    * find overlap between sets, and compare distances for each pair of old and new, and work out distance delta (delta = new - old)
    * for those where delta is negative (we've gotten closer) calculate velocity
    * emit prediction of wall-clock time we will pass each water crossing based on current distance to water crossing and current velocity towards it

We can run this predictor for each gps reading in each session, and then assess as follows:
* precision = for each water crossing, whenever we predicted that we would cross at time T_P, what was the actual T_A, and was it within some tolerance e.g. 30 seconds. count each of these as a boolean yes/no
* recall = for each water crossing that was ultimately passed in a session, did we make a prediction for it?

This measurement framework and predictor can both likely be improved, but we need to start with something.

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

### Tasks

...

## ...

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