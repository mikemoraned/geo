# Current Slice: Motis as ground truth for the UK

### Target

Replicate for the UK what the slice "Motis as ground truth" builds for Germany. A local Motis
server draws rail along the track and gives realtime arrival times. A week-long capture of the
whole country comes from it.

### Straw Man

Take [2026-10-23-great-britain-capture.md](2026-10-23-great-britain-capture.md) as the straw man.

### Refactor

The German setup moves from the top level of `tools/motis-server` into a subdirectory, so the
UK's setup can sit beside it. The Justfile, `motis_server`, and `geo_data` at that top level serve
Germany alone.

### Tasks

- [ ] Make the poller log a warning when a poll's share of realtime legs falls. The German capture
      lost its realtime on 20 October 2026, and nothing reported it, as
      [its caveat](completed-slices.md#caveat-realtime-stops-partway-through-the-capture) sets out.
- [ ] If the warning fires during a capture, investigate the cause before the capture ends. Compare
      the realtime feed's trip ids with the imported timetable's.
