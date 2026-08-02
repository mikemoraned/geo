# Current Slice: Bring data up-to-date + clarify documentation

### Target

This is a clean-up slice focussed on consolidating and improving what we have in data and docs.

### Tasks

#### Data

* [ ] add a small summary cli which goes over all the bronze silver and gold datasets and summaries what is in them
* [ ] do a clean local update in slice of all data to ensure we have a populated silver and gold area
  * [ ] re-download extracts from OvertureMaps
  * [ ] use summary cli to show what's in medallion now
* [ ] update bronze:
  * [ ] bring down latest data saved from devices that is in redis and regenerate silver/gold
  * [ ] use summary cli to show what's in medallion now
  * [ ] there is a new release of OvertureMaps available. Do new extracts that we need of water, stations etc, based on this new data, and regenerate silver/gold
  * [ ] use summary cli to show what's in medallion now

#### Docs

* [ ] remove unneeded documentation e.g. 
  * there is no need to add documentation to every usage of a library in Cargo.toml; only highlight things that are particularly unusual
* [ ] enable `writing-clearly-and-concisely` SKILL and do a pass over all existing documentation correcting it; ensure this SKILL is always enabled and on from now on
* [ ] extract knowledge out of claude-specific areas like .claude files and into explicit docs under docs:
  * These should be written in a style that passes `writing-clearly-and-concisely` SKILL and in additionaly should be focussed on key facts in a dispassionate technical style
  * This includes in .claude/memory:
    * m5-esp32-toolchain.md
    * lookout-architecture.md
    * motis-trips-api.md
  * This documentaion should additionally extract and remove details from "### Notes & Gotchas (what the on-device GNSS actually behaves like)" in next-slices.md
