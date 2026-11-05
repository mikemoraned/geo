use chrono::{DateTime, Utc};

#[derive(Debug, PartialEq, Eq)]
pub struct CandidatePass {
    since: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PredictedPass {
    since: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ActualPass {
    since: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateKind {
    OutOfReach,
    CandidatePass,
    PredictedPass,
    ActualPass,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CrossingState {
    CandidatePass(CandidatePass),
    PredictedPass(PredictedPass),
    ActualPass(ActualPass),
}

impl CandidatePass {
    pub fn new(at: DateTime<Utc>) -> Self {
        Self { since: at }
    }

    pub fn since(&self) -> DateTime<Utc> {
        self.since
    }

    pub fn predict(self, at: DateTime<Utc>) -> PredictedPass {
        PredictedPass { since: at }
    }
}

impl PredictedPass {
    pub fn new(at: DateTime<Utc>) -> Self {
        Self { since: at }
    }

    pub fn since(&self) -> DateTime<Utc> {
        self.since
    }

    pub fn pass(self, at: DateTime<Utc>) -> ActualPass {
        ActualPass { since: at }
    }

    pub fn withdraw(self, at: DateTime<Utc>) -> CandidatePass {
        CandidatePass { since: at }
    }
}

impl ActualPass {
    pub fn since(&self) -> DateTime<Utc> {
        self.since
    }

    pub fn predict(self, at: DateTime<Utc>) -> PredictedPass {
        PredictedPass { since: at }
    }

    pub fn release(self, at: DateTime<Utc>) -> CandidatePass {
        CandidatePass { since: at }
    }
}

impl CrossingState {
    pub fn kind(&self) -> StateKind {
        match self {
            Self::CandidatePass(_) => StateKind::CandidatePass,
            Self::PredictedPass(_) => StateKind::PredictedPass,
            Self::ActualPass(_) => StateKind::ActualPass,
        }
    }

    pub fn since(&self) -> DateTime<Utc> {
        match self {
            Self::CandidatePass(state) => state.since(),
            Self::PredictedPass(state) => state.since(),
            Self::ActualPass(state) => state.since(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn later(seconds: i64) -> DateTime<Utc> {
        instant() + TimeDelta::seconds(seconds)
    }

    #[test]
    fn a_crossing_coming_into_reach_is_a_candidate_since_then() {
        assert_eq!(CandidatePass::new(instant()).since(), instant());
    }

    #[test]
    fn a_crossing_out_of_reach_can_be_predicted_at_once() {
        assert_eq!(PredictedPass::new(instant()).since(), instant());
    }

    #[test]
    fn a_candidate_predicted_is_predicted_since_the_prediction() {
        let predicted = CandidatePass::new(instant()).predict(later(10));

        assert_eq!(predicted.since(), later(10));
    }

    #[test]
    fn a_prediction_that_comes_true_is_a_pass_since_the_pass() {
        let passed = PredictedPass::new(instant()).pass(later(10));

        assert_eq!(passed.since(), later(10));
    }

    #[test]
    fn a_prediction_withdrawn_is_a_candidate_again() {
        let withdrawn = PredictedPass::new(instant()).withdraw(later(10));

        assert_eq!(withdrawn.since(), later(10));
    }

    #[test]
    fn a_crossing_passed_can_be_predicted_again() {
        let again = PredictedPass::new(instant())
            .pass(later(10))
            .predict(later(20));

        assert_eq!(again.since(), later(20));
    }

    #[test]
    fn a_crossing_passed_and_released_is_a_candidate_again() {
        let released = PredictedPass::new(instant())
            .pass(later(10))
            .release(later(20));

        assert_eq!(released.since(), later(20));
    }

    #[test]
    fn a_crossing_state_is_since_the_state_it_holds() {
        let states = [
            CrossingState::CandidatePass(CandidatePass::new(later(1))),
            CrossingState::PredictedPass(PredictedPass::new(later(2))),
            CrossingState::ActualPass(PredictedPass::new(instant()).pass(later(3))),
        ];

        let sinces: Vec<_> = states.iter().map(CrossingState::since).collect();

        assert_eq!(sinces, vec![later(1), later(2), later(3)]);
    }
}
