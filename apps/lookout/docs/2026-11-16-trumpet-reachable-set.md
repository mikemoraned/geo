# The trumpet, as a reachable set

*Research of 2026-11-16, with nothing built. This doc places the trumpet from
[next-slices.md](next-slices.md#a-trumpet-around-the-heading) among the fields that study the same
shape, and names their terms. It is for whoever builds the trumpet. It also derives the shape from
the physics of a train in a curve, as a bound to compare a learned trumpet with.*

## The idea in one paragraph

LightCone keeps an alert on a crossing whose bearing lies within a fixed half-width of the
heading. The trumpet makes the half-width depend on the speed and on the distance to the crossing.
A fast train cannot take a tight curve, so a crossing near it and well off the heading is out of
reach. Further out, the train has room to turn, so the half-width grows.

## The general term: a reachable set

Control theory calls the set of states a system can reach from a starting state, under bounded
inputs, its **reachable set**, or **forward reachable set**. The study of it is **reachability
analysis**. LightCone's cone and the trumpet each approximate the positions in the reachable set of
a train, cut off at the horizon.

A vehicle that moves forward at a fixed speed and turns no tighter than a minimum radius is a
**Dubins car**. Its shortest paths, the **Dubins paths**, join arcs of that minimum radius with
straight lines. The boundary of its reachable set at a given time is traced by turning at the
maximum rate, then switching direction or running straight
([Wolfram, *3D Reachability Set for a Dubins Car*](https://demonstrations.wolfram.com/3DReachabilitySetForADubinsCar/)).
Curves of points reached at equal times are **isochrons**
([Wolfram, *Isochrons for a Dubins Car*](https://demonstrations.wolfram.com/IsochronsForADubinsCar/)).
The mathematical literature studies the set at a fixed time
([*Reachable set at a certain time for a Dubins car*](https://geodesic.mathdoc.fr/item/TIMM_2018_24_1_a12)).

A train on track is a Dubins car whose minimum radius depends on its speed. The trumpet is the
edge of its reachable set, seen from the train as a bearing.

## Why the radius depends on the speed

A train at speed `v` on a curve of radius `R` needs a centripetal acceleration of `v²/R`. Railway
engineering splits this between two terms:

- **Cant**, or **superelevation**: the height of the outer rail above the inner one, which tilts
  the train into the curve.
- **Cant deficiency**, or **unbalanced superelevation**: the further cant the track would need for the
  train's speed to be its **balancing speed**. At that speed, the force on the wheels is normal to
  the track.
  Passengers feel it as uncompensated lateral acceleration.

The permitted speed on a curve follows from both terms. With `g` for gravity, `E` for the cant,
`D` for the cant deficiency, and `G` for the distance between the rails' centres, about 1,500 mm
on standard gauge:

```
v_max = sqrt(g · R · (E + D) / G)
```

([Wikipedia, *Cant (road and rail)*, "Application to maximum permitted speed"](https://en.wikipedia.org/wiki/Cant_(road_and_rail)#Application_to_maximum_permitted_speed)).
Turned round, a train at speed `v` is on a curve no tighter than:

```
R_min(v) = v² · G / (g · (E + D)) = v² / a
```

where `a = g · (E + D) / G` is the largest lateral acceleration the track and the train allow.
The minimum radius grows with the square of the speed.

The limits vary by country and train. France and Germany allow up to 152 mm of cant deficiency on
conventional lines, and the TGV 102 mm
([Wikipedia, *Cant deficiency*, "Europe"](https://en.wikipedia.org/wiki/Cant_deficiency#Europe)).
European high-speed lines carry up to 180 mm of cant
([Wikipedia, *Cant (road and rail)*, "Superelevation in railway tracks"](https://en.wikipedia.org/wiki/Cant_(road_and_rail)#Superelevation_in_railway_tracks)).
EN 13803 sets the European limits. Neither Wikipedia page states the UK's. With 150 mm of each,
`a` is about 2 m/s².

Track also changes curvature gradually, through **transition curves**, which lengthen as the line
speed rises
([Wikipedia, *Cant (road and rail)*](https://en.wikipedia.org/wiki/Cant_(road_and_rail)#Superelevation_in_railway_tracks)).
A train therefore enters a curve over some distance, and turns less in a short window than the
minimum radius allows.

## The shape that follows

A train that turns at its minimum radius `R` from the start reaches the furthest bearing off its
heading. After an arc of length `s`, its heading has changed by `s / R`. Its bearing from the
start, the chord's angle, has changed by half that. At straight-line distance `d`, that bearing is:

```
half_width(d, v) = arcsin(d / (2 · R_min(v)))      for d < 2 · R_min(v)
```

At `d = 2 · R_min` the train has turned a half circle, and the half-width reaches 90°. Beyond it,
the bound allows any bearing ahead of the train.

For a small `d`, the half-width is about `d · a / (2 · v²)`. It grows linearly with distance, and
falls with the square of the speed. On a map, the lateral offset of the edge grows with the square
of the distance, so the outline flares like the bell of a trumpet.

With `a` at 2 m/s², the bound gives these half-widths:

| speed | `R_min` | at 500 m | at 1 km |
| --- | --- | --- | --- |
| 10 m/s (36 km/h) | 50 m | 90° | 90° |
| 20 m/s (72 km/h) | 200 m | 90° | 90° |
| 30 m/s (108 km/h) | 450 m | 34° | 90° |
| 50 m/s (180 km/h) | 1,250 m | 12° | 24° |
| 80 m/s (288 km/h) | 3,200 m | 4° | 9° |

The current alerting radius is 1 km. Within it, the physical bound is narrower than LightCone's
90° only above about 30 m/s. Track runs far straighter than this bound allows, which is why the
slice proposes to learn the relationship from the sessions. The bound serves as a sanity check on
what the learning finds: a learned half-width narrower than the bound is plausible, and a wider one
points to heading error.

Two effects widen the trumpet beyond the bound:

- **Heading error.** The heading comes from the fixes, and its error adds a constant angle at every
  distance. The trumpet's mouth is then this angle wide, not a point.
- **Braking.** A train slows before a curve, so its current speed bounds its curvature only near
  it. A service brake of about 0.7 m/s² takes a train from 50 m/s to 33 m/s over 1 km. The
  bound from the current speed then holds only for the first part of the alerting radius. The
  European Train Control System calls the planned slowing ahead of a speed restriction a
  **braking curve**.

## The neighbouring terms

### Gating, from target tracking

A tracker predicts where a target goes next and accepts only measurements near that prediction.
The region it accepts is the **validation gate**, and the test is **gating**. A gate grows as the
prediction looks further ahead. Trackers of manoeuvring targets often use a **coordinated turn
model**, in which the target moves at constant speed and constant **turn rate**. A turn rate `ω`
relates to the lateral acceleration by `ω = a / v`, so a bound on `a` gives a bound on the turn
rate that falls with speed. The trumpet is a gate on crossings rather than on measurements.

### Set-based prediction, from automated driving

[SPOT](https://commonroad.in.tum.de/tools/spot), by Koschi and Althoff at TU Munich, predicts the
**occupancy** of other road users: the set of positions each can reach, under physical limits and
traffic rules. It bounds acceleration, and so bounds turning at speed. Its sets over-approximate
by design, so that a planner avoids every position another road user can reach. The trumpet has
the opposite aim, precision over recall. The literature on these sets also uses **feasible set**,
and **kinematically feasible** for a trajectory a vehicle model allows
([*Boundary-Guided Trajectory Prediction*](https://arxiv.org/html/2505.06740v1)).

### The electronic horizon, from navigation

Driver-assistance systems publish an **electronic horizon**: the paths ahead of the vehicle, read
from a map, with the **most probable path** among them. The ADASIS protocol carries it to the
vehicle's systems. A provider assumes the current road continues as the most probable path until
the first junction
([US patent 9,566,982](https://patentimages.storage.googleapis.com/2f/1e/bf/2aa53633d5ba24/US9566982.pdf)).
[Horizon.KOM](https://www.kom.tu-darmstadt.de/papers/BPB16.pdf) is an open provider built on
OpenStreetMap. This is the graph-traversal route that
[the observational predictor notes](2026-10-03-observational-predictor.md) set against
observation. The trumpet stays geometric: it bounds where the track can go, without reading it.

## Shapes with other causes

Two terms name a similar outline for a different reason:

- **Cone of uncertainty.** The US National Hurricane Center's forecast cone widens with lead time
  because past track errors grow with lead time. It holds the storm's centre about two-thirds of
  the time
  ([Penn State, *Operational Forecasting Products from the National Hurricane Center*](https://courses.ems.psu.edu/meteo241/node/640)).
  Its width comes from error, where the trumpet's comes from kinematics. A learned trumpet mixes
  the two, since heading error widens it as well.
- **Funnel.** In robot motion planning, a funnel is the region a controlled system stays in along
  a trajectory, guaranteed by a Lyapunov function
  ([Majumdar and Tedrake, *Funnel libraries*](https://groups.csail.mit.edu/robotics-center/public_papers/Majumdar16.pdf)).
  A funnel narrows as feedback converges on the trajectory. The trumpet widens.

LightCone takes its name from physics, where a light cone holds every event a signal at light
speed can reach. Its edge has a fixed slope because light has one speed. The trumpet's edge bends
because a train's turning limit varies with its speed.

## What this means for the slice

- **Measure curvature, not the turning angle.** A turning angle depends on the window it is
  measured over. Curvature, the change of heading per metre, does not, and the bound gives its
  limit directly as `a / v²`. Learning the largest curvature at each speed, as a high quantile,
  gives `R_min(v)`, and the half-width follows from the formula in
  [The shape that follows](#the-shape-that-follows).
- **Or learn the half-width directly.** The sessions give, for each crossing passed, its bearing
  off the heading, its distance, and the speed at each earlier sample. A high quantile of the
  bearing, by distance and speed, is the trumpet itself. It takes in heading error and braking
  without a model of either.
- **Add the heading error as the mouth.** The width at zero distance is the heading error, a
  constant the curvature model leaves out.
- **Name it after its concept.** "Trumpet" names the outline. "Reachable set" or "turn bound"
  names what it bounds, as LightCone names its concept.
