# Current Slice: Evaluation framework with associated Crow Flies predictor improvement

### Target

I have some larger ideas for how to come up with a good predictor (see [notes](2026-10-03-observational-predictor.md)), but for now I want to:
* improve the predictor in a minimal way
* measure that improvement robustly

I'd like to go from a very simple Crow Flies predictor I have now, which effectively just shows distance to nearby crossings, to one that takes into account current speed and average direction, and predicts when a crossing will be passed in next N minutes. Effectively a "Light Cone" approach.

The evaluation method should be focussed on being well-understood and robust, even if it does not simulate all aspects of reality. More complete extensions can be reserved for later slices.

### Straw Man

#### What is a Prediction?

We have to become a lot crisper than previously on what a prediction means. Right now the display just shows a radar style view which isn't really making a prediction and is instead just a map of nearby crossings. 

My suggestion is that, for each crossing, we need to have a state-machine, something like:

States:
* `OutOfReach`: this is maybe an implicit state that any crossing sits in, if it is just in the pointset data, and isn't reachable at all
* `CandidatePass`: this is when it becomes nearby enough to be considered as a possible crossing
* `PredictedPass`: this is when we expect we will actually pass by this crossing in the next N minutes
* `ActualPass`: we just actually passed the crossing

Transitions:
* `OutOfReach` <-> `CandidatePass`: a crossing can oscillate in and out of being a candidate
* `OutOfReach` -> `PredictedPass` or `CandidatePass` -> `PredictedPass`: this is us making a prediction, and this transition should lead to a notification of some kind to the user
* `PredictedPass` -> `CandidatePass` or `PredictedPass` -> `OutOfReach`: this counts as a failed prediction
* `PredictedPass` -> `ActualPass`: a successful prediction.
  * A crossing stays in the `ActualPass` state until we've gotten some distance away from it to avoid re-notifying repeatedly.
  * In general, the state transitions should probably have some sort of debouncing or hysteresis to avoid frequent flips in and out of states.
* `ActualPass` -> `PredictedPass`: it's always possible we can pass somewhere multiple times, so this counts as a new prediction
* `ActualPass` -> `CandidatePass`: still within reach but not currently predicted
* `ActualPass` -> `OutOfReach`: it's now some distance from us and we don't even think it is a candidate

In this framework, depending on how clever the predictor is, we may be able to display a crossing but still label it as OutOfReach. This could be, for example, if we decide it is technically close by but we are not going to reach it because it's not on our current track. However, any predictor that can do that is likely out of scope of this slice.

#### Moving from a CrowFlies to a LightCone Predictor

The cleverness of the predictor we do in this slice should be limited to using a current-speed/direction + Light-cone approach i.e. predict what we will reach based on where we will pass by based on current velocity vector.

This can start with a low-level of cleverness about current speed/direction (i.e. just literal current speed) and then start building up an overall speed/direction based on an average over last few fixes and/or on speed reported by the GPS device.

I suspect on Web we could benefit from the cleverness of the GPS baked into the phone, as it's probably doing something clever, but I'd prefer not to use that and instead do it ourselves, as then we have consistent performance across platforms. I am thinking here of some sort of Kalman-filter or similar; however that's possibly over-kill for this slice.

#### Evaluation method and modelling approach

The method should be informed by the [research](2026-08-01-evaluation.md) done previously.

We should drive a predictor based on fake train data as if it was receiving it live, and then assess it based on ground truth of what was actually passed.

This should use the motis data we have recorded i.e. we should replay `train_session_sample_v2`, with some caveats. The intent of the previous slice was to record a full weeks worth of train data for Germany, but unfortunately part-way through we lost `realtime` feeds [without realising it](completed-slices.md#caveat-realtime-stops-partway-through-the-capture). 

Some [analysis](../notebooks/evaluation/explore1.py) shows which `trip_id`s on which `polled_date`s have > 90% coverage of realtime datapoints. It's about 40,000 trips clustered around 2026-10-18 and 2026-10-19. We should implement this 90% coverage filter by re-doing the derivation of `train_session_sample_v2` so that it contains a `realtime` column and extend `train_session_v2` to have a `realtime_pct` which allows us to apply the filter i.e. we'd only include sessions whose coverage was >= 90%.

The underlying data may require some further cleaning to remove any other anomalies that exist. As long as this doesn't massively affect coverage then we can adopt a bias to discard invalid data instead of trying to fix it. This may have to happen at the session level i.e. we may have to discard whole sessions if some percentage of it (e.g. 10%) is invalid.

We will also want to do a version of `session_crossing_v3` but for train sessions, `train_session_crossing_v1`. We'll need this to assess Precision i.e. what the predictor said it would reach soon, vs what it did.

To measure recall, one approach we could take is to have an "oracle" predictor to compare against: we could take the actual crossings the train passed and works backwards to when a perfect predictor would have said it was coming. This allows us to assemble a stream of crossing state updates that would be the best score. This may also flush out if there are some constraints or ambiguities in our system which means even an oracle with perfect foreknowledge can't get a perfect score on precision and recall.

Hopefully these `realtime` samples will be enough samples to give a good statistical evaluation, but if not, we can relax our `realtime` constraint and instead use all of the sessions we have.

We don't need to model gps accuracy in this slice, but it can be an extension we attempt if things go well. The sorts of thing we could do here would be to use the real recorded sessions from being on a train to build a distribution of gps accuracy and sample from that model. This could then be layered on top of the train position as an error signal to perturb it.

#### Visualising in Crux App on Web and M5

The CrowFlies and LightCone Predictors will need to make explicit the states of each crossing as they see them.

These only have to be minimally supported in the Crux Apps e.g. showing a different shape for each crossing either in the Canvas view, or as some ASCII-art on M5. They don't have to be updated to manifest a notification from the internal predictor as a user-visible notification.

#### Style of development and how to record evaluations

I generally prefer a Walking Skeleton approach and I'd like to apply that here, alongside some of the themes of explainability from [recent investigations](2026-10-18-demonstrable-behaviour.md). This means that I'd like to have a very basic evaluation in place, starting with a small amount of samples, say `1000`. This allows us to go through an inspectable series of steps that show if the method is working before extending to longer runs.

This also means we don't have to implement the LightCone predictor at start. That can come after we have a proper evaluation of CrowFlies in place.

The output of evaluation should live in `gold` as tables recording things like predictor name + parameters, evaluation method, sample sizes, and random seeds, as well as output. So, we probably want to have a mix of tables describing the evaluation run as well actual results. These can be summary distributions.

However, I probably also want the data in detail about what the predictor predicted and when, perhaps recorded as an `evaluation_session`, also in `gold`. This can perhaps record the state transitions over time. This can be inspected for debugging to check everything is working, and also to replay edge-cases or to visualise where there are hotspots of prediction failures. If we are wanting to replay over time, rerun may be a good choice, but a notebook is also fine.

Overall this means that Tasks should be broken down into major stages with each stage delivering some aspect of
* derivation of data for sessions
* driving predictor
* evaluating it
* visualising it
