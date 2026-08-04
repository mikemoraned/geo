# Current Slice: Crow-flies predictor deployed to M5 device and Rerun sim

### Target

One half of this is a clean-up / rationalisation of what we've already spiked on with the M5 device (apps/lookout/spikes/m5/spike7-battery-and-trend). The other half is turning rerun visualisation into something that shows what the predictor is doing.

Effectively what I want to end up with is:
* A core predictor defined in a CRUX wrapper, perhaps with its central core being a state-machine
* A productionised version of the M5-deployed setup that uses that core to read live GPS readings and predict when water will be crossed
* A rerun-based simulation that re-drives a named session (from silver/session table) as fake GPS readings through the predictor, captures the predictions, and visualises them
  * I want to use this as a way to see how the predictor is performing by live-comparing where it thinks it's going to cross water vs what water is actually there

As part of this we should also be able to delete all the current `visualise` code + remove the `spikes/m5` dir.
