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
    pub bbox: Rect<f64>,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Near {
    pub sample: Sample,
    pub distance_metres: f64,
    pub visit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Steps {
    pub near: Vec<Near>,
    pub passes: Vec<Pass>,
}

pub fn steps(
    session_id: &SessionId,
    samples: &[Sample],
    crossing: &Crossing,
    radius: Radius,
) -> Steps {
    let nearby = |sample: &Sample| distance(sample, crossing) <= radius.as_metres();
    let runs = samples.chunk_by(|a, b| nearby(a) == nearby(b));
    let visits: Vec<&[Sample]> = runs.filter(|run| nearby(&run[0])).collect();
    let near: Vec<Near> = visits
        .iter()
        .enumerate()
        .flat_map(|(visit, visit_samples)| {
            visit_samples.iter().map(move |sample| Near {
                sample: *sample,
                distance_metres: distance(sample, crossing),
                visit,
            })
        })
        .collect();
    let near_by_visit = near.chunk_by(|a, b| a.visit == b.visit);
    let passes = near_by_visit
        .filter_map(|visit| pass_of(session_id, crossing, visit))
        .collect();
    Steps { near, passes }
}

fn passes_of(session: &Session, crossings: &[Crossing], radius: Radius) -> Vec<Pass> {
    let reachable = grown(session.bbox, radius);
    crossings
        .iter()
        .filter(|crossing| contains(&reachable, crossing.crossing.position))
        .flat_map(|crossing| steps(&session.session_id, &session.samples, crossing, radius).passes)
        .collect()
}

fn pass_of(session_id: &SessionId, crossing: &Crossing, visit: &[Near]) -> Option<Pass> {
    let nearest = visit
        .iter()
        .min_by(|a, b| a.distance_metres.total_cmp(&b.distance_metres))?;

    Some(Pass {
        session_id: session_id.clone(),
        crossing_id: crossing.crossing.id.clone(),
        crossed_at: nearest.sample.t,
        distance_metres: nearest.distance_metres,
        samples_within: visit.len().try_into().unwrap_or(u32::MAX),
    })
}

fn distance(sample: &Sample, crossing: &Crossing) -> f64 {
    Euclidean.distance(sample.projected, crossing.projected)
}

fn grown(bbox: Rect<f64>, radius: Radius) -> Rect<f64> {
    use geo::{Destination, Haversine};

    let metres = radius.as_metres();
    let south_west = Haversine.destination(bbox.min().into(), 180.0, metres);
    let south_west = Haversine.destination(south_west, 270.0, metres);
    let north_east = Haversine.destination(bbox.max().into(), 0.0, metres);
    let north_east = Haversine.destination(north_east, 90.0, metres);

    Rect::new(south_west.0, north_east.0)
}

fn contains(bbox: &Rect<f64>, point: Point<f64>) -> bool {
    (bbox.min().x..=bbox.max().x).contains(&point.x())
        && (bbox.min().y..=bbox.max().y).contains(&point.y())
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
        let bbox = bbox_of(&samples);
        Session {
            session_id: SessionId::new("session-a").unwrap(),
            device_id: DeviceId::new("device-a").unwrap(),
            bbox,
            samples,
        }
    }

    fn bbox_of(samples: &[Sample]) -> Rect<f64> {
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
    fn a_session_reversing_over_a_crossing_passes_it_once_per_visit() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 200.0),
            sample(2, 400.0),
            sample(3, 200.0),
            sample(4, 0.0),
        ]);

        let passed = passes(&[session], &[crossing("c", 200.0)], Radius::new(50.0));

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.crossed_at)
                .collect::<Vec<_>>(),
            [at(1), at(3)]
        );
    }

    #[test]
    fn each_visit_counts_its_own_samples() {
        let session = session(vec![
            sample(0, 190.0),
            sample(1, 200.0),
            sample(2, 210.0),
            sample(3, 500.0),
            sample(4, 200.0),
        ]);

        let passed = passes(&[session], &[crossing("c", 200.0)], Radius::new(50.0));

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.samples_within)
                .collect::<Vec<_>>(),
            [3, 1]
        );
    }

    #[test]
    fn the_steps_number_each_near_sample_by_its_visit() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 190.0),
            sample(2, 210.0),
            sample(3, 400.0),
            sample(4, 200.0),
        ]);

        let steps = steps(
            &session.session_id,
            &session.samples,
            &crossing("c", 200.0),
            Radius::new(50.0),
        );

        assert_eq!(
            steps
                .near
                .iter()
                .map(|near| (near.sample.t, near.visit))
                .collect::<Vec<_>>(),
            [(at(1), 0), (at(2), 0), (at(4), 1)]
        );
        assert!((steps.near[1].distance_metres - 10.0).abs() < 0.001);
    }

    #[test]
    fn the_steps_end_in_the_passes_the_matching_records() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 200.0),
            sample(2, 400.0),
            sample(3, 200.0),
        ]);
        let crossing = crossing("c", 200.0);

        let steps = steps(
            &session.session_id,
            &session.samples,
            &crossing,
            Radius::new(50.0),
        );

        assert_eq!(steps.passes.len(), 2);
        assert_eq!(
            steps.passes,
            passes(&[session], &[crossing], Radius::new(50.0))
        );
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
