use domain::Gps;
use redis::aio::MultiplexedConnection;
use server::queue::{QUEUE_KEY, RedisSink, SampleSink};
use shared::{Accel, AccelReading, GpsReading, Message, V1Message};
use telemetry::RawSample;
use test_redis::TestRedis;
use uuid::Uuid;

#[tokio::test]
async fn redis_sink_lpushes_samples_in_fifo_order_docker() {
    let redis = TestRedis::start().await;
    let mut conn = redis.connection();

    let sink = RedisSink::new(redis.connection());

    let accel_sample = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(1),
        t: 1_700_000_000_001,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }));
    let gps_sample = Message::Version1(V1Message::Gps(GpsReading {
        id: Uuid::from_u128(2),
        t: 1_700_000_000_002,
        gps: Gps::at(55.95, -3.19)
            .expect("on the globe")
            .with_accuracy_metres(Some(8.5))
            .with_speed_mps(Some(31.4))
            .with_heading_degrees(Some(275.0)),
    }));

    let accel_item = wrap(&accel_sample);
    let gps_item = wrap(&gps_sample);

    let depth_after_accel = sink.push(&accel_item).await.expect("push accel");
    let depth_after_gps = sink.push(&gps_item).await.expect("push gps");
    assert_eq!((depth_after_accel, depth_after_gps), (1, 2));

    let len: i64 = redis::cmd("LLEN")
        .arg(QUEUE_KEY)
        .query_async(&mut conn)
        .await
        .expect("llen");
    assert_eq!(len, 2);

    let oldest = rpop_sample(&mut conn).await;
    let next_oldest = rpop_sample(&mut conn).await;
    assert_eq!(oldest.parse().expect("parse accel"), accel_sample);
    assert_eq!(next_oldest.parse().expect("parse gps"), gps_sample);
}

fn wrap(message: &Message) -> RawSample {
    RawSample::new(
        1_700_000_050_000,
        serde_json::to_string(message).expect("serialize"),
    )
}

async fn rpop_sample(conn: &mut MultiplexedConnection) -> RawSample {
    let item: String = redis::cmd("RPOP")
        .arg(QUEUE_KEY)
        .query_async(conn)
        .await
        .expect("rpop");
    serde_json::from_str(&item).expect("deserialize envelope")
}
