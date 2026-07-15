# Current Slice: mike is on a train getting data

### Target

The main thing I want to get is some real sensor data from being on a real train journey:
* periodic gps snapshot
* accelerometer data

### Architecture

I think the minimum thing required is:
1. A browser frontend that periodically samples gps and accelerometer data, and sends them on a websocket to backend service, stamped with a timestamp, and random. The frontend should generate a random uuid which it uses as it's identity, if it doesn't already have one persisted in a cookie. It should send this id on all samples. Samples should be simply JSON.
2. A rust web server:
    * providing the basic web-page for front-end
    * listening on the websocket and saving the telemetry data to a redis queue
3. A rust cli which can listen on this same queue and empty it in to a rerun.io file
4. This should then be visualisable in rerun

We should re-use as much style of implementation as used in https://github.com/mikemoraned/bobby
