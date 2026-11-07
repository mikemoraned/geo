use domain::StateKind;

#[derive(Debug, PartialEq, Eq)]
pub struct CandidatePass;

#[derive(Debug, PartialEq, Eq)]
pub struct AlertedPass;

#[derive(Debug, PartialEq, Eq)]
pub struct ActualPass(());

#[derive(Debug, PartialEq, Eq)]
pub enum CrossingState {
    Candidate(CandidatePass),
    Alerted(AlertedPass),
    Actual(ActualPass),
}

impl CandidatePass {
    pub fn alert(self) -> AlertedPass {
        AlertedPass
    }
}

impl AlertedPass {
    pub fn pass(self) -> ActualPass {
        ActualPass(())
    }

    pub fn withdraw(self) -> CandidatePass {
        CandidatePass
    }
}

impl ActualPass {
    pub fn alert(self) -> AlertedPass {
        AlertedPass
    }

    pub fn release(self) -> CandidatePass {
        CandidatePass
    }
}

impl CrossingState {
    pub fn kind_of(state: Option<&Self>) -> StateKind {
        match state {
            None => StateKind::OutOfReach,
            Some(Self::Candidate(_)) => StateKind::CandidatePass,
            Some(Self::Alerted(_)) => StateKind::AlertedPass,
            Some(Self::Actual(_)) => StateKind::ActualPass,
        }
    }
}
