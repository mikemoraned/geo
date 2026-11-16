use std::collections::VecDeque;
use std::num::NonZeroU32;

use domain::{Age, Sample};
use geo_types::Point;

use crate::Float;

const WINDOW: Age = Age::minutes(NonZeroU32::new(2).expect("two is not zero"));

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Trace(VecDeque<Sample<Float>>);

impl Trace {
    pub(crate) fn followed(&mut self, fix: Sample<Float>) {
        let since = WINDOW.before(fix.t);
        self.0.push_back(fix);
        self.0.retain(|kept| kept.t >= since);
    }

    pub(crate) fn positions(&self) -> impl Iterator<Item = Point<Float>> + '_ {
        self.0.iter().map(|fix| fix.gps.position)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};

    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
    }

    fn fix(seconds: i64, latitude: f64) -> Sample<Float> {
        Sample::<f64>::at(instant() + TimeDelta::seconds(seconds), latitude, 13.7322)
            .expect("on the globe")
            .to_precision()
            .expect("within f32")
    }

    fn latitudes(trace: &Trace) -> Vec<Float> {
        trace.positions().map(|position| position.y()).collect()
    }

    #[test]
    fn each_fix_joins_the_trace_in_turn() {
        let mut trace = Trace::default();

        trace.followed(fix(0, 51.0));
        trace.followed(fix(10, 51.1));

        assert_eq!(latitudes(&trace), vec![51.0, 51.1]);
    }

    #[test]
    fn a_fix_more_than_two_minutes_before_the_latest_leaves_the_trace() {
        let mut trace = Trace::default();

        trace.followed(fix(0, 51.0));
        trace.followed(fix(60, 51.1));
        trace.followed(fix(121, 51.2));

        assert_eq!(latitudes(&trace), vec![51.1, 51.2]);
    }

    #[test]
    fn a_fix_two_minutes_before_the_latest_stays() {
        let mut trace = Trace::default();

        trace.followed(fix(0, 51.0));
        trace.followed(fix(120, 51.2));

        assert_eq!(latitudes(&trace), vec![51.0, 51.2]);
    }
}
