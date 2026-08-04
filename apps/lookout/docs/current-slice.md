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

The sort of thing I am thinking of is a CRUX shell which defines the unit that is designed to be embedded in an M5 device, a rerun runner, or (later) an App or Website. For the M5 version of this in the spikes so far we've allowed the Events to contain GNSS strings from an attached GPS device. That's probably too low-level for what I want here.

Instead I'd like the shell to consume a normalised GPS Sample which is rich enough to represent all the information that may be useful. We can then create a parser for the GNSS strings which converts to this normalised form. We wouldn't need this parser code for the rerun version of this as it can be driven by fake normalised events e.g. that are extracted from `sessions` in silver.

Within this shell there would then be a state-machine which is even simpler in interface i.e.  it is effectively a state-machine that
* receives a series of Events which are either GPS Samples or a clock advancement to a timestamp (a GPS Sample also contains an embedded timestamp)
* transitions when an Event is passed
* can be queried to say which water passing points it predicts we will pass and when

This is minimal generic `trait` but additionally a state may expose extra info like, for example, which water passing points are receding or coming closer.

The Event type given to the state-machine can be same as given to the CRUX shell, but doesn't have to be.

For the purposes of producing a rerun simulation it may be easier to wrap and expose a state-machine rather than a full CRUX shell.

We should split the crates into a core area (current `crates` dir) and create a new `platform` sub-dir where `m5plus` and `rerun` can live. The esp platforms will need to be their own workspaces as they can't compile as part of the main workspace, but `rerun` probably can. 