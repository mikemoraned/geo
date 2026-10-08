mod common;

use chrono::{Duration, Utc};
use common::{RAIL_MODES, captured_segments, gps, lpush, start_redis, wait_ready};
use medallion::Root;
use motis::bronze::SegmentLog;
use motis::capture::Capture;
use motis::client::MotisClient;
use motis::near_gps::NearGps;

#[tokio::test]
async fn recent_gps_near_frankfurt_captures_rail_from_local_motis_end_to_end() {
    let (_redis, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let near_frankfurt_hbf = [
        (now_ms - 60_000, 50.107, 8.663),
        (now_ms - 30_000, 50.110, 8.660),
        (now_ms, 50.113, 8.669),
    ];
    for (id, (t, lat, lon)) in near_frankfurt_hbf.into_iter().enumerate() {
        lpush(&mut conn, &gps(id as u128, t, lat, lon)).await;
    }

    let store = tempfile::tempdir().expect("temp store");
    let mut near_gps = NearGps::new(Duration::minutes(30), Duration::minutes(5), 1000);
    let mut capture = Capture::new(
        MotisClient::default(),
        SegmentLog::new(Root::new(store.path()), common::local_source().id()),
        8.0,
        Duration::minutes(5),
    );

    let area = near_gps
        .look(now, &mut conn)
        .await
        .expect("look")
        .area
        .expect("an area around recent GPS");
    let captured = capture
        .capture(now, &area)
        .await
        .expect("capture against local motis");

    assert!(
        captured.segments > 0,
        "expected some rail segments near Frankfurt Hbf"
    );

    let rows = captured_segments(&Root::new(store.path())).await;

    assert!(
        rows.iter().all(|s| RAIL_MODES.contains(&s.mode.as_str())),
        "every captured segment should be a rail mode; got {:?}",
        rows.iter().map(|s| s.mode.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rows.iter().any(|s| s.agency_name.is_some()),
        "expected at least one segment's trip to resolve an agency"
    );
    assert!(
        rows.iter().any(|s| s.train_number.is_some()),
        "expected at least one train number from the /trip enrichment"
    );
}
