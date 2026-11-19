use chrono::{DateTime, Utc};
use domain::{Precision, Sample};
use predictor::{Velocity, VelocityEstimator};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VelocityAt<P: Precision> {
    pub t: DateTime<Utc>,
    pub velocity: Option<Velocity<P>>,
}

pub fn velocities<P: Precision, V: VelocityEstimator<P>>(
    samples: impl IntoIterator<Item = Sample<P>>,
    mut estimator: V,
) -> Vec<VelocityAt<P>> {
    samples
        .into_iter()
        .map(|sample| {
            estimator.observe(sample);
            VelocityAt {
                t: sample.t,
                velocity: estimator.velocity(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use domain::Radius;
    use predictor::LastMove;

    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn fix_at(latitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, 0.0).expect("on the globe")
    }

    fn estimated(samples: &[Sample<f64>]) -> Vec<VelocityAt<f64>> {
        velocities(
            samples.iter().copied(),
            LastMove::new(Radius::new(20.0).expect("a radius")),
        )
    }

    fn speeds_and_headings(estimated: &[VelocityAt<f64>]) -> Vec<Option<(f64, f64)>> {
        estimated
            .iter()
            .map(|at| {
                at.velocity.map(|velocity| {
                    (
                        (velocity.speed_mps * 100.0).round() / 100.0,
                        (velocity.heading_degrees * 10.0).round() / 10.0,
                    )
                })
            })
            .collect()
    }

    #[test]
    fn no_samples_give_no_velocities() {
        assert_eq!(estimated(&[]), vec![]);
    }

    #[test]
    fn each_sample_gives_the_velocity_estimated_once_it_is_observed() {
        let samples = [fix_at(50.0, 0), fix_at(50.01, 10), fix_at(50.01, 20)];

        let estimated = estimated(&samples);

        assert_eq!(
            estimated.iter().map(|at| at.t).collect::<Vec<_>>(),
            vec![
                instant(),
                instant() + TimeDelta::seconds(10),
                instant() + TimeDelta::seconds(20)
            ]
        );
        assert_eq!(
            speeds_and_headings(&estimated),
            vec![None, Some((111.2, 0.0)), Some((55.6, 0.0))]
        );
    }
}
