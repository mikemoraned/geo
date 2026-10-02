use chrono::{DateTime, TimeZone, Utc};
use domain::{Bbox, CrossingId, DeviceId, SessionId, StartedBy};
use geo_types::{LineString, Point};
use medallion::{Country, GeoRow, Root, write_geo_rows, write_rows};
use medallion_model::{SessionCrossingRow, SessionRow, SessionSampleRow};
use session_crossings::gold::{Choosing, choose};

const BERLIN: (f64, f64) = (13.404954, 52.520008);
const EDINBURGH: (f64, f64) = (-3.188267, 55.953251);

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 22, 9, minute, 0).unwrap()
}

fn session_of(named: &str, at_lon_lat: (f64, f64)) -> (SessionRow, Point<f64>) {
    let (lon, lat) = at_lon_lat;
    let session = SessionRow {
        session_id: SessionId::new(named).expect("a session id"),
        device_id: DeviceId::new(format!("device-{named}")).expect("a device id"),
        started_at: at(0),
        ended_at: at(30),
        sample_count: 1,
        started_by: StartedBy::Gap,
        gap_seconds: 600,
        lead_seconds: 60,
        bbox: Bbox::new(lon, lat, lon, lat).expect("a bbox"),
    };
    (session, Point::new(lon, lat))
}

struct Recorded<'a> {
    country: Country,
    at: (f64, f64),
    sessions: &'a [(&'a str, usize)],
}

async fn store_with(root: &Root, recorded: &[Recorded<'_>]) {
    let mut session_rows = Vec::new();
    let mut sample_rows = Vec::new();
    let mut crossed = Vec::new();

    for Recorded {
        country,
        at: at_lon_lat,
        sessions,
    } in recorded
    {
        for (named, crossings) in *sessions {
            let (session, point) = session_of(named, *at_lon_lat);
            let session_id = session.session_id.clone();
            let device_id = session.device_id.clone();
            session_rows.push(GeoRow {
                row: session,
                geometry: LineString::from(vec![(point.x(), point.y()), (point.x(), point.y())]),
                country: *country,
            });
            sample_rows.push(GeoRow {
                row: SessionSampleRow {
                    session_id: session_id.clone(),
                    device_id: device_id.clone(),
                    t: at(1),
                    seq: 0,
                    lat: point.y(),
                    lon: point.x(),
                    alt: None,
                    acc: 5.0,
                    speed: None,
                    heading: None,
                    implied_speed_mps: None,
                },
                geometry: point,
                country: *country,
            });
            for n in 0..*crossings {
                crossed.push(SessionCrossingRow {
                    session_id: session_id.clone(),
                    crossing_id: CrossingId::new(format!("water:track:rail@{named}-{n}"))
                        .expect("a crossing id"),
                    device_id: device_id.clone(),
                    crossed_at: at(2 + n as u32),
                    distance_m: 10.0,
                    samples_within: 1,
                    match_radius_m: 250.0,
                });
            }
        }
    }

    write_geo_rows(root, &session_rows)
        .await
        .expect("write the sessions");
    write_geo_rows(root, &sample_rows)
        .await
        .expect("write the samples");
    write_rows(root, &crossed).await.expect("write the passes");
}

#[tokio::test]
async fn a_country_with_fewer_than_the_cap_contributes_what_it_has() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    store_with(
        &root,
        &[
            Recorded {
                country: Country::Germany,
                at: BERLIN,
                sessions: &[("de-best", 6), ("de-next", 5), ("de-third", 4)],
            },
            Recorded {
                country: Country::UnitedKingdom,
                at: EDINBURGH,
                sessions: &[("gb-only", 4)],
            },
        ],
    )
    .await;

    let replays = choose(
        &root,
        Choosing {
            min_crossings: 3,
            max_sessions: 2,
        },
    )
    .await
    .expect("choose the replays");

    let chosen: Vec<String> = replays
        .iter()
        .map(|replay| replay.session.to_string())
        .collect();
    assert_eq!(chosen, ["de-best", "de-next", "gb-only"]);
}

#[tokio::test]
async fn a_session_passing_too_few_crossings_is_left_out_of_its_country() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    store_with(
        &root,
        &[Recorded {
            country: Country::Germany,
            at: BERLIN,
            sessions: &[("de-enough", 5), ("de-too-few", 2)],
        }],
    )
    .await;

    let replays = choose(
        &root,
        Choosing {
            min_crossings: 3,
            max_sessions: 5,
        },
    )
    .await
    .expect("choose the replays");

    let chosen: Vec<String> = replays
        .iter()
        .map(|replay| replay.session.to_string())
        .collect();
    assert_eq!(chosen, ["de-enough"]);
}
