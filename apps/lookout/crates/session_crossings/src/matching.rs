//! Matching a session's samples against the crossings it came near.
//!
//! The rule is pure distance: a crossing was passed in a session if any sample of that session
//! came within the match radius of it, and the sample that came nearest says when. That is
//! deliberately simple, and its known failure is a crossing on a line running parallel to the
//! one travelled: within the radius, so recorded as passed, though it never was. Fixing that
//! means matching the session to track rather than to points, which is a piece of work in its
//! own right and is not needed to get a first precision and recall number.
//!
//! Two numbers travel with each match so a reader can weigh it: how far the nearest sample
//! was, and **how many** samples fell inside the radius. One sample within the radius and
//! twenty are different evidence that a crossing was really passed — a session that never
//! moved can produce the first without having gone anywhere.

use chrono::{DateTime, Utc};
use geo::{Distance, Euclidean};
use geo_types::{Point, Rect};
use model::{CrossingId, DeviceId, SessionCrossingRow, SessionId};

/// How near a sample has to come to a crossing for the crossing to count as passed.
///
/// Metres, so it is read against the projected geometry — the reason silver carries one.
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
    /// Where the nearest-sample distances stop looking like crossings that were passed.
    ///
    /// Their distribution has two parts: a decay from zero, which is a crossing actually gone
    /// over — seen from however far the previous fix happened to be, since a train at 100 km/h
    /// sampled every ten seconds leaves 280 m between fixes — and, beyond it, a flat spread
    /// that is the density of crossings near a path rather than crossings on it. The default
    /// is where the first ends; the evidence for the value is in the slice notes.
    fn default() -> Self {
        Self(250.0)
    }
}

/// One session as this matches it: where it went, and when it was at each point.
///
/// Positions are projected metres, and the envelope is the session's own in lat/lon — the
/// column the store carries so that "which sessions could have come near this place" is
/// answerable without opening their samples.
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    pub envelope: Rect<f64>,
    pub samples: Vec<Sample>,
}

/// One sample of a session: when it was taken, and where in metres.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub t: DateTime<Utc>,
    pub at: Point<f64>,
}

/// One crossing as this matches against it: in metres for the distance, and in lat/lon for
/// the envelope prune.
#[derive(Debug, Clone)]
pub struct Crossing {
    pub crossing_id: CrossingId,
    pub at: Point<f64>,
    pub lat_lon: Point<f64>,
}

/// The crossings each session passed, as the rows of the ground truth.
///
/// Every session is matched against only the crossings inside its own envelope, grown by the
/// radius, so the distance is computed for the pairs that could possibly be within it rather
/// than for every pair. The rows come back ordered by instant, which is the order they are
/// partitioned and written in.
pub fn passes(
    sessions: &[Session],
    crossings: &[Crossing],
    radius: Radius,
) -> Vec<SessionCrossingRow> {
    let mut passed: Vec<SessionCrossingRow> = sessions
        .iter()
        .flat_map(|session| passes_of(session, crossings, radius))
        .collect();
    passed.sort_by_key(|pass| (pass.crossed_at, pass.crossing_id.to_string()));
    passed
}

/// The crossings one session passed.
fn passes_of(session: &Session, crossings: &[Crossing], radius: Radius) -> Vec<SessionCrossingRow> {
    let reachable = grown(session.envelope, radius);
    crossings
        .iter()
        .filter(|crossing| contains(&reachable, crossing.lat_lon))
        .filter_map(|crossing| passed(session, crossing, radius))
        .collect()
}

/// One crossing as a session passed it, or `None` where no sample came within the radius.
fn passed(session: &Session, crossing: &Crossing, radius: Radius) -> Option<SessionCrossingRow> {
    let within: Vec<(f64, &Sample)> = session
        .samples
        .iter()
        .map(|sample| (Euclidean.distance(sample.at, crossing.at), sample))
        .filter(|(distance, _)| *distance <= radius.as_metres())
        .collect();

    let (distance_m, nearest) = within
        .iter()
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .copied()?;

    Some(SessionCrossingRow {
        session_id: session.session_id.clone(),
        crossing_id: crossing.crossing_id.clone(),
        device_id: session.device_id.clone(),
        crossed_at: nearest.t,
        distance_m,
        samples_within: within.len().try_into().unwrap_or(u32::MAX),
        match_radius_m: radius.as_metres(),
    })
}

/// `envelope` grown by `radius` in every direction, on the sphere rather than by treating a
/// degree as a fixed distance — a degree of longitude is a different length at every latitude.
fn grown(envelope: Rect<f64>, radius: Radius) -> Rect<f64> {
    use geo::{Destination, Haversine};

    let metres = radius.as_metres();
    let south_west = Haversine.destination(envelope.min().into(), 180.0, metres);
    let south_west = Haversine.destination(south_west, 270.0, metres);
    let north_east = Haversine.destination(envelope.max().into(), 0.0, metres);
    let north_east = Haversine.destination(north_east, 90.0, metres);

    Rect::new(south_west.0, north_east.0)
}

/// Whether `point` falls within `envelope`, edges included.
///
/// `Rect`'s own containment excludes its edges, and a crossing exactly on the grown edge is
/// one at exactly the radius, which the distance test does count.
fn contains(envelope: &Rect<f64>, point: Point<f64>) -> bool {
    (envelope.min().x..=envelope.max().x).contains(&point.x())
        && (envelope.min().y..=envelope.max().y).contains(&point.y())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    /// Berlin, in lat/lon and in the zone Germany's projected geometry uses.
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

    /// The samples' own envelope in lat/lon, standing in for the one the store carries.
    ///
    /// The samples here are laid out in metres east of Berlin, so a metre is converted at the
    /// latitude they sit at — near enough for an envelope, which the distance test decides
    /// within anyway.
    fn envelope_of(samples: &[Sample]) -> Rect<f64> {
        let degrees = |metres: f64| metres / 111_320.0 / f64::cos(BERLIN.1.to_radians());
        let east = |sample: &Sample| BERLIN.0 + degrees(sample.at.x() - BERLIN_METRES.0);
        let min = samples.iter().map(east).fold(f64::MAX, f64::min);
        let max = samples.iter().map(east).fold(f64::MIN, f64::max);
        Rect::new((min, BERLIN.1), (max, BERLIN.1))
    }

    /// A sample `east` metres east of Berlin, taken at `minute`.
    fn sample(minute: u32, east: f64) -> Sample {
        Sample {
            t: at(minute),
            at: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
        }
    }

    /// A crossing `east` metres east of Berlin.
    fn crossing(id: &str, east: f64) -> Crossing {
        let degrees = east / 111_320.0 / f64::cos(BERLIN.1.to_radians());
        Crossing {
            crossing_id: CrossingId::new(id).unwrap(),
            at: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
            lat_lon: Point::new(BERLIN.0 + degrees, BERLIN.1),
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

    /// The instant recorded is the nearest sample's, not the first one inside the radius:
    /// the nearest is the best evidence of when the crossing was actually reached.
    #[test]
    fn the_nearest_sample_says_when_the_crossing_was_passed() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 200.0)], Radius::new(150.0));

        assert_eq!(passed[0].crossed_at, at(2));
        assert!((passed[0].distance_m - 20.0).abs() < 0.001, "{passed:?}");
    }

    /// How many samples fell inside the radius is what separates a session that ran past a
    /// crossing from one that produced a single fix near it.
    #[test]
    fn every_sample_inside_the_radius_is_counted() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], Radius::new(150.0));

        assert_eq!(passed[0].samples_within, 3);
    }

    /// One row per `(session, crossing)`, however many samples came within the radius.
    #[test]
    fn a_crossing_passed_by_many_samples_is_one_row() {
        let session = session((0..10).map(|i| sample(i, f64::from(i) * 10.0)).collect());

        let passed = passes(&[session], &[crossing("c", 50.0)], Radius::new(150.0));

        assert_eq!(passed.len(), 1);
    }

    /// A sample exactly at the radius counts: the radius is the furthest a sample may be,
    /// and the envelope grown by it must therefore keep the crossing too.
    #[test]
    fn a_crossing_at_exactly_the_radius_was_passed() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], Radius::new(100.0));

        assert_eq!(passed.len(), 1);
    }

    /// The tuning a row was matched under travels with it, since a match made at 150 m and
    /// one made at 20 m are not the same claim.
    #[test]
    fn a_row_records_the_radius_it_was_matched_under() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 10.0)], Radius::new(100.0));

        assert_eq!(passed[0].match_radius_m, 100.0);
    }

    /// A session that never moved still passes a crossing it was parked beside, and says so
    /// with one sample: the row is written, and `samples_within` is what a reader weighs.
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
