//! How full the battery is, from what the shell measured.
//!
//! The shell reads a voltage; everything about what that voltage *means* is here, so it can be
//! tested against a battery running down — which on real hardware takes an hour and a half and
//! cannot be repeated on demand.
//!
//! The voltage-to-charge curve comes from `battery-estimator`, rather than being fitted here:
//! a lithium-polymer cell sits near 3.7 V for most of its life and then falls off a cliff, so
//! interpolating linearly between "empty" and "full" would read half-full for most of a
//! discharge and then drop three bars at once.

use battery_estimator::{BatteryChemistry, SocEstimator};

/// The cell in the PLUS2 is a lithium-polymer one, and the crate carries the curve for it:
/// 3.2 V empty, 3.7 V nominal, 4.2 V charged.
const ESTIMATOR: SocEstimator = SocEstimator::new(BatteryChemistry::LiPo);

/// What could plausibly be a battery on this board. **The estimator clamps rather than
/// refuses** — it has no out-of-range error — so a disconnected pin reading near zero would
/// come back as a confident 0%, and a misread of 9 V as a confident 100%. Neither is a
/// battery, and saying nothing is better than saying "full".
const PLAUSIBLE_VOLTS: core::ops::RangeInclusive<f32> = 3.0..=4.4;

/// How full, in as many steps as the indicator can honestly claim. The curve is flat through
/// the middle of a discharge, so finer steps would be reporting noise as information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Charge {
    Empty,
    Low,
    Half,
    Full,
}

impl Charge {
    /// The state-of-charge above which each step starts, as a percentage.
    const THRESHOLDS: [(Self, f32); 3] =
        [(Self::Full, 75.0), (Self::Half, 50.0), (Self::Low, 25.0)];
    /// How far past a threshold the charge has to go before the step changes. Without it a
    /// reading sitting on a boundary flickers between two steps every time it is taken, and
    /// the panel redraws on every change.
    const HYSTERESIS: f32 = 3.0;

    /// How many bars this fills, of [`Self::BARS`].
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

/// Turns measured voltages into a charge step, remembering the last one so that a reading
/// hovering on a boundary does not flicker.
#[derive(Debug, Default)]
pub struct Battery {
    charge: Option<Charge>,
}

impl Battery {
    /// Absorbs one reading. `None` while no reading has arrived, or if the voltage is outside
    /// anything the curve describes — a disconnected battery reads as nearly zero, and
    /// claiming "empty" for it would be a different lie from claiming nothing.
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

    /// The step `percent` falls in, keeping the current one until the reading has moved past
    /// its boundary by [`Charge::HYSTERESIS`].
    fn step(&self, percent: f32) -> Charge {
        let held = self.charge;

        Charge::THRESHOLDS
            .iter()
            .find(|(step, threshold)| {
                // A step already held keeps its place until the charge falls clear of it.
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

    /// A lithium-polymer cell: about 4.2 V charged, about 3.0 V flat.
    const CHARGED: u16 = 4_200;
    /// The curve calls 3.2V empty, so that is where "flat" is, not where the cell dies.
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

    /// The property that matters over a journey: it only ever goes down as the voltage does.
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

    /// A reading sitting on a boundary would otherwise flip on every measurement, and the
    /// panel redraws whenever the view changes.
    #[test]
    fn a_reading_hovering_on_a_boundary_does_not_flicker() {
        let mut battery = Battery::default();
        battery.measured(CHARGED);

        // Find a voltage that sits exactly where the top step gives way.
        let boundary = (FLAT..=CHARGED)
            .rev()
            .find(|millivolts| {
                let mut fresh = Battery::default();
                fresh.measured(*millivolts);
                fresh.charge() != Some(Charge::Full)
            })
            .expect("a boundary somewhere in the range");

        // Coming down onto it from full, the held step survives a wobble across the line.
        battery.measured(boundary + 5);
        battery.measured(boundary);
        assert_eq!(
            battery.charge(),
            Some(Charge::Full),
            "flipped at {boundary}mV"
        );
    }

    /// A disconnected battery reads near zero, which is not the same as a flat one, and the
    /// indicator should say nothing rather than say "empty".
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
