use std::num::NonZeroU32;

use domain::{Radius, Span};

pub const DEFAULT_WATCHING: Watching = Watching {
    radius: Radius::metres(NonZeroU32::new(5_000).expect("five thousand is not zero")),
};

pub const DEFAULT_ALERTING: Alerting = Alerting {
    radius: Radius::metres(NonZeroU32::new(1_000).expect("one thousand is not zero")),
    horizon: Span::minutes(NonZeroU32::new(5).expect("five is not zero")),
};

pub const DEFAULT_RADII: Radii = Radii::checked(DEFAULT_WATCHING, DEFAULT_ALERTING)
    .expect("the default alerting radius lies within the default watching radius");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Watching {
    pub radius: Radius,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alerting {
    pub radius: Radius,
    pub horizon: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error(
    "an alerting radius of {} m reaches beyond the watching radius of {} m",
    .alerting.as_metres(),
    .watching.as_metres()
)]
pub struct AlertingBeyondWatching {
    pub watching: Radius,
    pub alerting: Radius,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    watching: Watching,
    alerting: Alerting,
}

impl Radii {
    pub fn new(watching: Watching, alerting: Alerting) -> Result<Self, AlertingBeyondWatching> {
        Self::checked(watching, alerting).ok_or(AlertingBeyondWatching {
            watching: watching.radius,
            alerting: alerting.radius,
        })
    }

    pub const fn checked(watching: Watching, alerting: Alerting) -> Option<Self> {
        if alerting.radius.as_metres() > watching.radius.as_metres() {
            None
        } else {
            Some(Self { watching, alerting })
        }
    }

    pub const fn watching(self) -> Watching {
        self.watching
    }

    pub const fn alerting(self) -> Alerting {
        self.alerting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(metres: f64) -> Radius {
        Radius::new(metres).expect("a radius")
    }

    fn watching(radius_metres: f64) -> Watching {
        Watching {
            radius: radius(radius_metres),
        }
    }

    fn alerting(radius_metres: f64) -> Alerting {
        Alerting {
            radius: radius(radius_metres),
            horizon: Span::new("5m").expect("a span"),
        }
    }

    #[test]
    fn an_alerting_radius_beyond_the_watching_radius_is_refused() {
        let refused = Radii::new(watching(1_000.0), alerting(2_000.0));

        assert_eq!(
            refused,
            Err(AlertingBeyondWatching {
                watching: radius(1_000.0),
                alerting: radius(2_000.0),
            })
        );
    }

    #[test]
    fn an_alerting_radius_equal_to_the_watching_radius_is_accepted() {
        let accepted = Radii::new(watching(1_000.0), alerting(1_000.0));

        assert_eq!(
            accepted.map(|radii| (radii.watching(), radii.alerting())),
            Ok((watching(1_000.0), alerting(1_000.0)))
        );
    }

    #[test]
    fn an_alerting_radius_within_the_watching_radius_is_accepted() {
        let accepted = Radii::new(watching(2_000.0), alerting(1_000.0));

        assert_eq!(
            accepted.map(|radii| (radii.watching(), radii.alerting())),
            Ok((watching(2_000.0), alerting(1_000.0)))
        );
    }
}
