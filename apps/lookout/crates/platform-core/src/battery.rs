use battery_estimator::{BatteryChemistry, SocEstimator};

const ESTIMATOR: SocEstimator = SocEstimator::new(BatteryChemistry::LiPo);

const PLAUSIBLE_VOLTS: core::ops::RangeInclusive<f32> = 3.0..=4.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Charge {
    Empty,
    Low,
    Half,
    Full,
}

impl Charge {
    const THRESHOLDS: [(Self, f32); 3] =
        [(Self::Full, 75.0), (Self::Half, 50.0), (Self::Low, 25.0)];
    const HYSTERESIS: f32 = 3.0;

    pub fn bars(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Low => 1,
            Self::Half => 2,
            Self::Full => 3,
        }
    }

    pub const BARS: usize = 3;
}

#[derive(Debug, Default)]
pub struct Battery {
    charge: Option<Charge>,
}

impl Battery {
    pub fn charge(&self) -> Option<Charge> {
        self.charge
    }

    pub fn measured(&mut self, millivolts: u16) {
        let volts = f32::from(millivolts) / 1_000.0;
        let percent = PLAUSIBLE_VOLTS
            .contains(&volts)
            .then(|| ESTIMATOR.estimate_soc(volts).ok())
            .flatten();

        self.charge = percent.map(|percent| self.step(percent));
    }

    fn step(&self, percent: f32) -> Charge {
        let held = self.charge;

        Charge::THRESHOLDS
            .iter()
            .find(|(step, threshold)| {
                let leaving = held.is_some_and(|held| held >= *step);
                percent >= threshold - if leaving { Charge::HYSTERESIS } else { 0.0 }
            })
            .map(|(step, _)| *step)
            .unwrap_or(Charge::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARGED: u16 = 4_200;
    const FLAT: u16 = 3_200;

    fn after(readings: &[u16]) -> Option<Charge> {
        let mut battery = Battery::default();
        for reading in readings {
            battery.measured(*reading);
        }
        battery.charge()
    }

    #[test]
    fn nothing_is_claimed_before_a_reading_arrives() {
        assert_eq!(Battery::default().charge(), None);
    }

    #[test]
    fn a_charged_cell_reads_full_and_a_flat_one_empty() {
        assert_eq!(after(&[CHARGED]), Some(Charge::Full));
        assert_eq!(after(&[FLAT]), Some(Charge::Empty));
    }

    #[test]
    fn a_falling_voltage_never_reads_fuller() {
        let mut battery = Battery::default();
        let mut last = Charge::Full;

        for millivolts in (FLAT..=CHARGED).rev().step_by(10) {
            battery.measured(millivolts);
            let charge = battery.charge().expect("a reading in range");
            assert!(
                charge <= last,
                "{millivolts}mV read {charge:?} after {last:?}"
            );
            last = charge;
        }
    }

    #[test]
    fn every_step_is_reachable_somewhere_in_a_discharge() {
        let seen: Vec<Charge> = (FLAT..=CHARGED)
            .rev()
            .step_by(10)
            .scan(Battery::default(), |battery, millivolts| {
                battery.measured(millivolts);
                Some(battery.charge())
            })
            .flatten()
            .collect();

        for step in [Charge::Full, Charge::Half, Charge::Low, Charge::Empty] {
            assert!(seen.contains(&step), "{step:?} never appeared");
        }
    }

    #[test]
    fn a_reading_hovering_on_a_boundary_does_not_flicker() {
        let mut battery = Battery::default();
        battery.measured(CHARGED);

        let boundary = (FLAT..=CHARGED)
            .rev()
            .find(|millivolts| {
                let mut fresh = Battery::default();
                fresh.measured(*millivolts);
                fresh.charge() != Some(Charge::Full)
            })
            .expect("the voltage where the top step gives way");

        let wobbling_across_the_boundary = [boundary + 5, boundary];
        for millivolts in wobbling_across_the_boundary {
            battery.measured(millivolts);
        }
        assert_eq!(
            battery.charge(),
            Some(Charge::Full),
            "flipped at {boundary}mV"
        );
    }

    #[test]
    fn a_voltage_the_curve_does_not_describe_claims_nothing() {
        assert_eq!(after(&[0]), None);
        assert_eq!(after(&[9_000]), None);
    }

    #[test]
    fn each_step_fills_one_more_bar_than_the_last() {
        assert_eq!(Charge::Empty.bars(), 0);
        assert_eq!(Charge::Full.bars(), Charge::BARS);
        assert!(Charge::Low.bars() < Charge::Half.bars());
    }
}
