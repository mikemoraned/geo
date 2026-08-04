# Current Slice: Bring data up-to-date + clarify documentation

### Target

This is a clean-up slice focussed on consolidating and improving what we have in data and docs.

### Tasks

#### Data

* [x] add a small summary cli which goes over all the bronze silver and gold datasets and summaries what is in them (`just summarise`, `--partitions` for the breakdown)
* [x] change the `bronze-extract` recipe and the underlying `extract` cli so that it has two modes, probably as subcommands:
  * `extract backfill`: takes as an arg an existing `extract_id` (e.g. `20260727T193628Z`) and re-fetches that extract's associated release / country / window. With no id given, backfill the latest extract in the manifest
  * `extract new`: does a new extract, using the defaults as of now
  * the Justfile default should be `extract backfill`, so bare `just bronze-extract` backfills the latest
* [x] do a clean local update in slice of all data to ensure we have a populated silver and gold area
  * [x] re-download extracts from OvertureMaps
  * [x] use summary cli to show what's in medallion now
* [x] update bronze and regenerate silver/gold:
  * [x] bring down latest data saved from devices that is in redis
  * [x] regenerate silver/gold
  * [x] use summary cli to show what's in medallion now
  * [x] there is a new release of OvertureMaps available. Do new extracts that we need of water, stations etc, based on this new data
  * [x] regenerate silver/gold
  * [x] use summary cli to show what's in medallion now

#### Docs

* [x] remove unneeded documentation e.g. 
  * there is no need to add documentation to every usage of a library in Cargo.toml; only highlight things that are particularly unusual
    * find other examples like this and ask, per category, if they should be deleted
  * remove duplication e.g. we don't need docs for a cli in both the Justfile entry and in the cli code; choose one
* [x] enable `writing-clearly-and-concisely` SKILL and do a pass over all existing documentation correcting it; ensure this SKILL is always enabled and on from now on
  * always-on is a rule in the repo `CLAUDE.md`, which every session loads, along with the
    two deliberate departures from the skill: British spelling, and `data` as a mass noun
  * `completed-slices.md` was included in the pass by request — wording only, never the
    record of what was done, since `.claude/rules/slices.md` holds it append-only
* [x] enable `technical-writing@technical-writing` skill
  ```
  /plugin marketplace add rnorth/technical-writing
  /plugin install technical-writing@technical-writing
  ```
* [ ] do a pass over docs with `technical-writing@technical-writing` skill
* [ ] ensure this `technical-writing@technical-writing` skill is always enabled
* [x] extract knowledge out of claude-specific areas like .claude files and into explicit docs under docs:
  * landed as three topic docs beside `medallion.md`: `device.md`, `motis.md`, `architecture.md`
  * `docs/` is self-contained and makes no reference to the spikes, so they can be deleted
    without losing anything; the pfaedle blocker went to `next-slices.md` as a parked slice,
    and the Motis server facts were already in `tools/motis-server/Justfile`
  * These should be written in a style that passes `writing-clearly-and-concisely` SKILL and additionaly should be focussed on key facts in a dispassionate technical style
  * This includes in .claude/memory:
    * m5-esp32-toolchain.md
    * lookout-architecture.md
    * motis-trips-api.md
  * This documentation should additionally extract and remove details from:
    * "### Notes & Gotchas (what the on-device GNSS actually behaves like)" in next-slices.md
    * the various READMEs in each spike
  * The overall intent is to end up with one place where we record factual information we have discovered
