# Next Slices

## Slice: bootstrap getting sensor data and saving it in https://rerun.io format

### Target

We want to get the most minimal setup of:
* local server running in rust serving a web page
* that web page is loaded in Safari and accessess the accelerometer (see https://developer.mozilla.org/en-US/docs/Web/API/Accelerometer)
* accelerometer data is sent back to the listening rust server over a websocket connection
* rust server saves that data to disk in rerun.io format
* data is loaded in rerun.io app and visualised

### Decisions

The crates layout should be very similar to https://github.com/mikemoraned/bobby. Feel free to copy / be-inspired by that layout.

### Tasks

* see target.md for overall guiding advice, but this initially breaks down into:
    * [ ] a spike that gets this working at all
        * ...
    * [ ] a refactoring that introduces the crux library as a refactoring that splits the parts that are working into the correct architectural area

## Slice: get iroh running on m5stack and on laptop, sharing accelerometer data

### Target

See "dumb accelerometer" idea in target.md. If iroh doesn't work, we can always share over BLE.

### Tasks 

...
