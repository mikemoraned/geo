# An observational predictor, bucketed by cell

*An idea of 2026-10-03, from a conversation, with nothing built. This doc records an approach to
the predictor, the facts that bound it, and the advice that held up under scrutiny. The predictor
today is the straw man in [target.md](target.md). It times every crossing within a radius at the
current speed, and ignores both the line and the direction of travel. A finding that survives
measurement moves to [target.md](target.md) and [architecture.md](architecture.md), and the rest
goes stale.*

## The idea

The crow-flies predictor times every crossing within a radius at the current speed. It ignores
which line the train is on and which way it faces. The replacement respects topology and timing.
It predicts only the crossings reachable from the current line, and only those reached within a
time horizon T.

Two routes lead there:

- **Graph traversal.** Encode the track topology, and traverse it forward from the current
  position. Timetables and expected speeds, layered on the graph, decide what is reachable and
  when.
- **Observation.** Skip the causal model. Record many journeys from Motis or a similar source,
  each as a series of samples over time, and bucket the samples into H3 cells. From these, learn
  a distribution: *a train in this cell, at this speed, in this direction, passed a water
  crossing within time T in X% of cases.* Collapse that distribution into a static map from
  (speed, direction, cell) to the reachable crossings and their arrival-time distributions. The
  live predictor is then a lookup.

## What each route is good at

The two routes are strong in opposite places, and the project's hardest cases fall on one side.

| | Graph traversal | Observation |
| --- | --- | --- |
| Models | the world (structure) | the observations (behaviour) |
| Junctions | explicit: the branches are known | weak, as [Junctions](#junctions-the-multimodality-is-in-the-set-not-the-timing) describes |
| Direction | inherent in the traversal | must be in the key |
| Unseen routes, diversions, engineering works | generalises | silent or wrong |
| Real dwell, real routing, real delay | assumed, unless fed live data | captured with no extra work |
| Inference cost | a live search | a hash and a read |
| Upkeep | build and maintain the graph | retrain as the network drifts |

The graph knows structure and is weak on behaviour. Observation knows behaviour and is weak on
structure. Junctions, disruption, and sparse lines are the cases that matter, and on each of them
observation is weakest and the graph strongest.

## The key is (speed, direction, cell), and the fixes it needs

Keying on (speed, direction, cell), each bucketed and rounded, is sound. It removes the simplest
failure: on a two-way single line, the up and down trains land in separate buckets instead of
averaging into one. It also makes the live query depend only on what the vehicle observes of
itself: its position, speed, and heading. At inference time it needs no trip or route. The map
becomes a purely geometric lookup.

The rounding hides four problems, each with a fix:

- **Direction wraps.** 359° and 1° are 2° apart, but a naive bucket puts them either side of a
  boundary. Bucket modularly, and average headings as unit vectors, not as degrees.
- **Heading is undefined at low speed.** A train dwelling, or crawling through points, has no
  meaningful bearing. Add a stationary bucket, below a speed threshold, that ignores direction.
  Near-zero-speed headings then stay out of the map.
- **Direction varies across a cell.** An H3 cell at resolution 9 has an edge of about 170 m, and
  a mainline curve sweeps several degrees across it. Use the exit bearing, since it predicts the
  downstream crossing. Use the same bearing in training and inference, or the buckets do not line
  up.
- **Hard bucket edges are brittle.** Two near-identical physical states can land in different
  buckets. Soft or overlapping buckets, or a light kernel over neighbouring buckets, steady the
  lookup near boundaries.

**Train on a richer key than the query uses.** The samples carry trip and route identity, as
[Motis does not give what the observation route assumes](#motis-does-not-give-what-the-observation-route-assumes)
describes. Build the map conditioned on `(route, direction_id, …)`, which resolves the junction
split. Then marginalise down to the thin (speed, direction, cell) key for the condensed live map.
Disambiguation happens in training, where identity is available. The thin map serves inference,
where identity is absent. The marginalised map inherits the junction multimodality again. That
multimodality is measurable per cell, as the entropy of the branch distribution, and the
measurement flags the cells where the thin key is unreliable.

## Junctions: the multimodality is in the set, not the timing

For a given crossing, the arrival-time distribution is unimodal in the normal case: one path, one
peak. It turns multimodal only when several paths reach the same crossing, and the time-horizon
cap prunes most of those.

The remaining multimodality sits one level up, in which crossings are in play at all. In a cell
approaching a junction, two trains with the same (speed, direction, cell) reach different sets of
crossings. The split shows as set membership, not as a lumpy distribution. Crossing A appears at
p≈0.6 and crossing B at p≈0.4, each with its own unimodal timing. The horizon cap does not remove
the split. It limits only how far past the junction the split reaches. The predictor reports the
split, showing both crossings with their branch probabilities, rather than bucketing it away.

## Motis does not give what the observation route assumes

The observation route assumes a stream of real positions, speeds, and headings. The feed behind
Motis gives something else, as [motis.md](motis.md) establishes:

- **No German open feed gives vehicle positions.** Motis interpolates a train's position along
  the timetabled leg's polyline, and corrects it by the realtime delay. The position is never a
  reported GPS fix, so the samples hold derived positions. Speed and direction differentiated
  from them follow partly from the cell and the shape, rather than carrying independent signal.
  On this feed, the advantage of learning real behaviour over the timetable is only partly real.
- **Rail geometry is straight lines.** Motis loads `shapes.txt`, but only bus and coach operators
  populate it. Rail legs come back as polylines of four points or fewer. The interpolation
  therefore runs along straight chords between stops, not along the curved track. An
  observational map built from Motis rail learns chord geometry, not track geometry. A cell- and
  topology-aware predictor depends on exactly that geometry. Curved rail needs shapes synthesised
  by map-matching against OSM, which the parked pfaedle slice in [next-slices.md](next-slices.md)
  covers.
- **Delay is the live variable worth modelling.** DELFI static with DELFI RT resolves about
  99.96% of trip updates. About 80% of segments in a city-sized box come back
  realtime-corrected. Beyond the timetable, Motis offers less a position-to-crossing map than an
  empirical distribution of delay and dwell. That distribution corrects a scheduled arrival at
  each crossing.

The same facts carry an advantage. Every observation is tied to a `trip_id`, with an ordered stop
sequence and a `direction_id`, the latter from the second `/api/v4/trip` call. Direction and
branch are therefore known, not inferred. That makes training on the rich key and marginalising
down feasible.

## The synthesis this points to

Use the graph for reachability and observation for timing. The graph answers which crossings lie
topologically ahead, and on which branch. It prunes the impossible and handles junctions
explicitly. Without a graph, the device's own clamp-to-line answers instead. The samples supply
calibrated arrival distributions, covering delay, dwell, and speed regime, over that candidate
set. The split keeps observation's one real advantage, behaviour beyond the timetable. It hands
direction, junctions, and sparse lines to the graph, which handles them well.

Where no graph exists, a purely observational predictor remains viable as a fallback. It needs
three things to account for its weaknesses:

- a hierarchical back-off from fine to coarse cells where samples are sparse
- an explicit confidence output when a bucket is thin
- staleness detection as the network and its services drift

## What to decide or measure first

Ranked by information gained per unit of effort.

1. **Decide the position source.** The live input is the device's own GPS, a real position. The
   Motis training corpus is interpolation along straight rail chords. Training and inference
   would then use different geometries, so this decision gates the rest. There are three options:
   - Map-match the rail with pfaedle, so samples lie on real track.
   - Train on real GPS traces.
   - Use a graph for reachability, and Motis only for delay and dwell.
2. **Measure the junction multimodality.** On the rich `(route, direction_id, cell)` map, compute
   the branch entropy of each cell. The entropy shows how far, and where, the thin (speed,
   direction, cell) key misleads. It also shows whether a purely observational predictor is
   tenable without a graph.
3. **Measure sparsity against resolution.** On one corridor, count the samples in each
   (cell × speed × direction) bucket at two or three H3 resolutions. The counts decide between a
   single resolution and a hierarchical back-off. They also set the horizon T against how far a
   bucket's samples reach.

## A note on the source

Volunteers run the public Transitous endpoint, with no SLA. Its usage policy asks that projects be
open source and non-commercial, and keep traffic light. It also asks them to send a real
User-Agent with contact details, and to make contact before routine use of routing endpoints. A
strategy of polling continuously to build a corpus needs clearing with Transitous first. The
alternative is to take the raw GTFS and GTFS-RT feeds. For Germany the policy does not apply: the
project runs its own `tools/motis-server` on DELFI. Transitous serves as the pan-European
aggregator for areas the project does not feed itself.

## References

- [target.md](target.md): the straw-man predictor as built (radius, current speed, no line), and
  the goal it stops short of
- [motis.md](motis.md): no vehicle positions, interpolated position, straight-line rail shapes,
  DELFI realtime resolution, and the second call for trip identity
- [next-slices.md](next-slices.md): the parked pfaedle map-matching slice, which would give
  curved rail shapes
- [Transitous](https://transitous.org/) and its [API](https://transitous.org/api/): the community
  MOTIS instance, its usage policy, and the MOTIS 2 endpoint
- [MOTIS](https://github.com/motis-project/motis): input formats (GTFS, GTFS-RT, …) and the REST
  API
- [H3](https://h3geo.org/): the hierarchical hex grid, including the edge length at each
  resolution, for the trade-off between sparsity and resolution
