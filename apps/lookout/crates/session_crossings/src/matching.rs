use chrono::{DateTime, Utc};
use domain::{DeviceId, Pass, SessionId};
use geo::{Distance, Euclidean};
use geo_types::{Point, Rect};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Radius(f64);

impl Radius {
    pub fn new(metres: f64) -> Self {
        Self(metres)
    }

    pub fn as_metres(self) -> f64 {
        self.0
    }
}

impl Default for Radius {
    fn default() -> Self {
        Self(250.0)
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    pub envelope: Rect<f64>,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub t: DateTime<Utc>,
    pub projected: Point<f64>,
}

#[derive(Debug, Clone)]
pub struct Crossing {
    pub crossing: domain::Crossing,
    pub projected: Point<f64>,
}

pub fn passes(sessions: &[Session], crossings: &[Crossing], radius: Radius) -> Vec<Pass> {
    let mut passed: Vec<Pass> = sessions
        .iter()
        .flat_map(|session| passes_of(session, crossings, radius))
        .collect();
    passed.sort_by(|a, b| (a.crossed_at, &a.crossing_id).cmp(&(b.crossed_at, &b.crossing_id)));
    passed
}

fn passes_of(session: &Session, crossings: &[Crossing], radius: Radius) -> Vec<Pass> {
    let reachable = grown(session.envelope, radius);
    crossings
        .iter()
        .filter(|crossing| contains(&reachable, crossing.crossing.position))
        .filter_map(|crossing| passed(session, crossing, radius))
        .collect()
}

fn passed(session: &Session, crossing: &Crossing, radius: Radius) -> Option<Pass> {
    let within: Vec<(f64, &Sample)> = session
        .samples
        .iter()
        .map(|sample| {
            (
                Euclidean.distance(sample.projected, crossing.projected),
                sample,
            )
        })
        .filter(|(distance, _)| *distance <= radius.as_metres())
        .collect();

    let (distance_metres, nearest) = within
        .iter()
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .copied()?;

    Some(Pass {
        session_id: session.session_id.clone(),
        crossing_id: crossing.crossing.id.clone(),
        crossed_at: nearest.t,
        distance_metres,
        samples_within: within.len().try_into().unwrap_or(u32::MAX),
    })
}

fn grown(envelope: Rect<f64>, radius: Radius) -> Rect<f64> {
    use geo::{Destination, Haversine};

    let metres = radius.as_metres();
    let south_west = Haversine.destination(envelope.min().into(), 180.0, metres);
    let south_west = Haversine.destination(south_west, 270.0, metres);
    let north_east = Haversine.destination(envelope.max().into(), 0.0, metres);
    let north_east = Haversine.destination(north_east, 90.0, metres);

    Rect::new(south_west.0, north_east.0)
}

fn contains(envelope: &Rect<f64>, point: Point<f64>) -> bool {
    (envelope.min().x..=envelope.max().x).contains(&point.x())
        && (envelope.min().y..=envelope.max().y).contains(&point.y())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const BERLIN: (f64, f64) = (13.404954, 52.520008);
    const BERLIN_METRES: (f64, f64) = (798_809.63, 5_828_000.60);

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 22, 9, minute, 0).unwrap()
    }

    fn session(samples: Vec<Sample>) -> Session {
        let envelope = envelope_of(&samples);
        Session {
            session_id: SessionId::new("session-a").unwrap(),
            device_id: DeviceId::new("device-a").unwrap(),
            envelope,
            samples,
        }
    }

    fn envelope_of(samples: &[Sample]) -> Rect<f64> {
        let degrees = |metres: f64| metres / 111_320.0 / f64::cos(BERLIN.1.to_radians());
        let east = |sample: &Sample| BERLIN.0 + degrees(sample.projected.x() - BERLIN_METRES.0);
        let min = samples.iter().map(east).fold(f64::MAX, f64::min);
        let max = samples.iter().map(east).fold(f64::MIN, f64::max);
        Rect::new((min, BERLIN.1), (max, BERLIN.1))
    }

    fn sample(minute: u32, east: f64) -> Sample {
        Sample {
            t: at(minute),
            projected: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
        }
    }

    fn crossing(id: &str, east: f64) -> Crossing {
        let degrees = east / 111_320.0 / f64::cos(BERLIN.1.to_radians());
        Crossing {
            crossing: domain::Crossing::at(
                domain::CrossingId::new(id).expect("a name"),
                BERLIN.1,
                BERLIN.0 + degrees,
            )
            .expect("on the globe"),
            projected: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
        }
    }

    #[test]
    fn a_crossing_the_session_came_within_the_radius_of_was_passed() {
        let session = session(vec![sample(0, 0.0), sample(1, 500.0)]);

        let passed = passes(&[session], &[crossing("c", 480.0)], Radius::new(100.0));

        assert_eq!(passed.len(), 1);
        assert_eq!(passed[0].crossing_id.to_string(), "c");
    }

    #[test]
    fn a_crossing_no_sample_came_near_was_not() {
        let session = session(vec![sample(0, 0.0), sample(1, 500.0)]);

        let passed = passes(&[session], &[crossing("c", 5_000.0)], Radius::new(100.0));

        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn the_nearest_sample_says_when_the_crossing_was_passed() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 200.0)], Radius::new(150.0));

        assert_eq!(passed[0].crossed_at, at(2));
        assert!(
            (passed[0].distance_metres - 20.0).abs() < 0.001,
            "{passed:?}"
        );
    }

    #[test]
    fn every_sample_inside_the_radius_is_counted() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], Radius::new(150.0));

        assert_eq!(passed[0].samples_within, 3);
    }

    #[test]
    fn a_crossing_passed_by_many_samples_is_one_row() {
        let session = session((0..10).map(|i| sample(i, f64::from(i) * 10.0)).collect());

        let passed = passes(&[session], &[crossing("c", 50.0)], Radius::new(150.0));

        assert_eq!(passed.len(), 1);
    }

    #[test]
    fn a_crossing_at_exactly_the_radius_was_passed() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], Radius::new(100.0));

        assert_eq!(passed.len(), 1);
    }

    #[test]
    fn a_session_of_one_sample_passes_what_it_sat_next_to() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 10.0)], Radius::new(100.0));

        assert_eq!(passed[0].samples_within, 1);
    }

    #[test]
    fn rows_come_back_in_the_order_they_were_crossed() {
        let session = session(vec![sample(0, 0.0), sample(5, 1_000.0)]);

        let passed = passes(
            &[session],
            &[crossing("late", 1_000.0), crossing("early", 0.0)],
            Radius::new(50.0),
        );

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.crossing_id.to_string())
                .collect::<Vec<_>>(),
            ["early", "late"]
        );
    }
}
