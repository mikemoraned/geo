# Current Slice: Crow-flies predictor deployed to M5 device and Rerun sim

### Target

One half of this is a clean-up / rationalisation of what we've already spiked on with the M5 device (apps/lookout/spikes/m5/spike7-battery-and-trend). The other half is turning rerun visualisation into something that shows what the predictor is doing.

Effectively what I want to end up with is:
* A core predictor defined in a CRUX wrapper, perhaps with its central core being a state-machine
* A productionised version of the M5-deployed setup that uses that core to read live GPS readings and predict when water will be crossed
* A rerun-based simulation that re-drives a named session (from silver/session table) as fake GPS readings through the predictor, captures the predictions, and visualises them
  * I want to use this as a way to see how the predictor is performing by live-comparing where it thinks it's going to cross water vs what water is actually there

As part of this we should also be able to delete all the current `visualise` code + remove the `spikes/m5` dir.

### Sketch

The sort of thing I am thinking of is a CRUX core which is designed to be embedded in a different shell; M5 device, a rerun runner, or (later) an App or Website.

For the M5 version of this in the spikes so far we've allowed the Events to contain GNSS strings from an attached GPS device. That's probably too low-level for what I want here. Instead I'd like the core to consume a normalised GPS Sample which is rich enough to represent all the information that may be useful. We can then create a parser for the GNSS strings which converts to this normalised form. We wouldn't need this parser code for the rerun version of this as it can be driven by fake normalised events e.g. that are extracted from `sessions` in silver.

Within this core there would then be a state-machine which is even simpler in interface i.e. it
* receives a series of Events which are either GPS Samples or a clock advancement to a timestamp (a GPS Sample also contains an embedded timestamp so can also advance time)
* transitions when an Event is passed
* can be queried to say which water passing points it predicts we will pass and when

This state-machine interface should be captured by a minimal generic `trait`. A particular state-machine struct implementing this trair may additionally expose extra info like, for example, which water passing points are receding or coming closer, for the sake of a nicer display. It may make sense to capture these expectations in a separate trait.

The Event type given to the state-machine can be same as given to the CRUX core, but doesn't have to be.

For the purposes of producing a rerun simulation it may be easier to wrap and expose a state-machine directly rather than a full CRUX core.

Under `crates` dir we should add a new `platform` sub-dir where `m5plus` and `rerun` can live. The esp platform will need to be in its own workspace as they can't compile as part of the main workspace, but `rerun` probably can.

### Tasks

We probably need to break this up into roughly these steps:
1. Get same functionality working as in `apps/lookout/spikes/m5/spike7-battery-and-trend` but in a productionised form following the `platform` setup.
  * note that now may be a good time to try out some of the higher-level crates we found for managing the M5 e.g. when looking for battery level exposing code
2. Extract a `rerun` platform
3. Delete all stuff no longer needed:
  * `visualise` stuff can go
  * so can all the `m5` spikes