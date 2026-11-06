use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, header};
use domain::Gps;
use futures_util::{SinkExt, StreamExt};
use server::auth::code::ShortCode;
use server::auth::memory::MemoryAuthStore;
use server::auth::sessions::SharedSessionStore;
use server::auth::store::{
    AllowedPasskey, AuthStore, Ceremony, CeremonyId, Insertion, PendingPasskey, StoreError, Update,
};
use server::auth::{Allowing, RelyingParty};
use server::queue::{PushError, SampleSink};
use server::{AppState, build_app};
use shared::{Accel, AccelReading, GpsReading, Message, V0Message, V1Message};
use telemetry::RawSample;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, advance, resume, sleep, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tower_sessions::MemoryStore;
use tower_sessions::cookie::Cookie;
use uuid::Uuid;
use webauthn_rs::prelude::{CredentialID, Url};

use crate::auth::{Browser, LOCALHOST, post, register, sign_in, static_dir};

struct RecordingSink {
    samples: Arc<Mutex<Vec<RawSample>>>,
}

#[async_trait::async_trait]
impl SampleSink for RecordingSink {
    async fn push(&self, sample: &RawSample) -> Result<i64, PushError> {
        let mut samples = self.samples.lock().expect("lock");
        samples.push(sample.clone());
        Ok(samples.len() as i64)
    }
}

#[derive(Default)]
struct Passkeys {
    store: MemoryAuthStore,
    lookups_fail: AtomicBool,
}

#[async_trait::async_trait]
impl AuthStore for Passkeys {
    async fn put_ceremony(&self, id: CeremonyId, ceremony: Ceremony) -> Result<(), StoreError> {
        self.store.put_ceremony(id, ceremony).await
    }

    async fn take_ceremony(&self, id: CeremonyId) -> Result<Option<Ceremony>, StoreError> {
        self.store.take_ceremony(id).await
    }

    async fn insert_pending(
        &self,
        code: &ShortCode,
        passkey: PendingPasskey,
    ) -> Result<Insertion, StoreError> {
        self.store.insert_pending(code, passkey).await
    }

    async fn take_pending(&self, code: &ShortCode) -> Result<Option<PendingPasskey>, StoreError> {
        self.store.take_pending(code).await
    }

    async fn find_allowed(&self, id: &CredentialID) -> Result<Option<AllowedPasskey>, StoreError> {
        match self.lookups_fail.load(Ordering::SeqCst) {
            true => Err(serde_json::from_str::<()>("")
                .expect_err("unparseable")
                .into()),
            false => self.store.find_allowed(id).await,
        }
    }

    async fn put_allowed(&self, passkey: AllowedPasskey) -> Result<(), StoreError> {
        self.store.put_allowed(passkey).await
    }

    async fn update_allowed(&self, passkey: AllowedPasskey) -> Result<Update, StoreError> {
        self.store.update_allowed(passkey).await
    }

    async fn list_allowed(&self) -> Result<Vec<AllowedPasskey>, StoreError> {
        self.store.list_allowed().await
    }

    async fn remove_allowed(&self, code: &ShortCode) -> Result<Option<AllowedPasskey>, StoreError> {
        self.store.remove_allowed(code).await
    }
}

struct Running {
    addr: SocketAddr,
    recorded: Arc<Mutex<Vec<RawSample>>>,
    app: Router,
    passkeys: Arc<Passkeys>,
}

async fn spawn_app() -> Running {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let origin = Url::parse(LOCALHOST).expect("an origin");
    let passkeys = Arc::new(Passkeys::default());
    let relying_party = RelyingParty::new(
        &origin,
        passkeys.clone(),
        SharedSessionStore::new(MemoryStore::default()),
        Allowing::AtOnce,
    )
    .expect("a relying party");
    let app = build_app(
        AppState {
            sink: Some(Arc::new(RecordingSink {
                samples: Arc::clone(&recorded),
            })),
            relying_party,
        },
        static_dir(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let served = app.clone();
    tokio::spawn(async move {
        axum::serve(listener, served).await.expect("serve");
    });
    Running {
        addr,
        recorded,
        app,
        passkeys,
    }
}

async fn signed_in(app: &Router) -> (CredentialID, Cookie<'static>) {
    let mut browser = Browser::at(LOCALHOST);
    let (passkey, _) = register(app, &mut browser).await;
    let cookie = sign_in(app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    (passkey, cookie)
}

async fn connect(
    addr: SocketAddr,
    cookie: Option<&Cookie<'_>>,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, WsError> {
    connect_from(addr, Some(LOCALHOST), cookie).await
}

async fn connect_from(
    addr: SocketAddr,
    origin: Option<&str>,
    cookie: Option<&Cookie<'_>>,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, WsError> {
    let mut request = format!("ws://{addr}/ws")
        .into_client_request()
        .expect("a request");
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().expect("a header"));
    }
    if let Some(cookie) = cookie {
        request.headers_mut().insert(
            header::COOKIE,
            cookie.stripped().to_string().parse().expect("a header"),
        );
    }
    connect_async(request).await.map(|(ws, _)| ws)
}

async fn acked(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> WsMessage {
    let sample = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(99),
        t: 1_700_000_000_099,
        accel: Accel {
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
        },
    }));
    let json = serde_json::to_string(&sample).expect("serialize");
    ws.send(WsMessage::Text(json.into())).await.expect("send");
    timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("a reply within 5s")
        .expect("stream open")
        .expect("frame")
}

async fn close_code(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> CloseCode {
    let closed = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("closed within 5s")
        .expect("a close frame")
        .expect("frame");
    let WsMessage::Close(Some(frame)) = closed else {
        panic!("{closed:?} is not a close frame");
    };
    frame.code
}

fn refused_status(
    connected: Result<WebSocketStream<MaybeTlsStream<TcpStream>>, WsError>,
) -> Option<StatusCode> {
    match connected {
        Err(WsError::Http(response)) => Some(response.status()),
        _ => None,
    }
}

#[tokio::test]
async fn both_versions_enqueue_and_malformed_is_dropped() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;

    let unversioned_from_the_archive = r#"{"id":"00000000-0000-0000-0000-000000000007","t":1700000000007,"gps":{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}}"#;
    let v0_expected = Message::Version0(V0Message::Gps(GpsReading {
        id: Uuid::from_u128(7),
        t: 1_700_000_000_007,
        gps: Gps::at(55.95, -3.19)
            .expect("on the globe")
            .with_accuracy_metres(Some(8.5)),
    }));

    let v1 = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(42),
        t: 1_700_000_000_042,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }));
    let v1_json = serde_json::to_string(&v1).expect("serialize");

    let mut ws = connect(running.addr, Some(&cookie)).await.expect("connect");
    ws.send(WsMessage::Text("not-a-sample".into()))
        .await
        .expect("send malformed");
    ws.send(WsMessage::Text(unversioned_from_the_archive.into()))
        .await
        .expect("send v0");
    ws.send(WsMessage::Text(v1_json.into()))
        .await
        .expect("send v1");
    ws.close(None).await.expect("close");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let samples = running.recorded.lock().expect("lock");
            if samples.len() >= 2 {
                assert_eq!(
                    samples.len(),
                    2,
                    "the malformed sample was sent before both valid ones, so two recorded \
                     means the handler reached it and dropped it"
                );
                assert_eq!(samples[0].parse().expect("parse v0"), v0_expected);
                assert_eq!(samples[1].parse().expect("parse v1"), v1);
                assert!(
                    samples[0].received_at() > 0,
                    "server stamps received_at at handling time"
                );
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "samples never reached the sink within 5s"
        );
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn server_acks_accepted_message() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;

    let mut ws = connect(running.addr, Some(&cookie)).await.expect("connect");

    assert_eq!(acked(&mut ws).await, WsMessage::Text("ack".into()));
}

#[tokio::test]
async fn an_upgrade_without_a_session_is_refused() {
    let running = spawn_app().await;

    let refused = refused_status(connect(running.addr, None).await);

    assert_eq!(refused, Some(StatusCode::UNAUTHORIZED));
}

#[tokio::test]
async fn an_upgrade_after_signing_out_is_refused() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;
    post(&running.app, "/auth/signout", Body::empty(), Some(&cookie)).await;

    let refused = refused_status(connect(running.addr, Some(&cookie)).await);

    assert_eq!(refused, Some(StatusCode::UNAUTHORIZED));
}

#[tokio::test]
async fn an_upgrade_from_another_origin_is_refused_even_when_signed_in() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;
    let others = [
        "https://localhost:3000",
        "http://localhost:3001",
        "http://localhost.example.com:3000",
        "http://evil.example.com",
        "null",
    ];

    let mut refusals = Vec::new();
    for other in others {
        refusals.push(refused_status(
            connect_from(running.addr, Some(other), Some(&cookie)).await,
        ));
    }

    assert_eq!(refusals, vec![Some(StatusCode::FORBIDDEN); others.len()]);
}

#[tokio::test]
async fn an_upgrade_without_an_origin_is_refused_even_when_signed_in() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;

    let refused = refused_status(connect_from(running.addr, None, Some(&cookie)).await);

    assert_eq!(refused, Some(StatusCode::FORBIDDEN));
}

#[tokio::test(start_paused = true)]
async fn revoking_a_passkey_closes_only_its_socket_within_a_minute() {
    let running = spawn_app().await;
    let (revoked, revoked_cookie) = signed_in(&running.app).await;
    let (_, kept_cookie) = signed_in(&running.app).await;
    let mut revoked_ws = connect(running.addr, Some(&revoked_cookie))
        .await
        .expect("connect");
    let mut kept_ws = connect(running.addr, Some(&kept_cookie))
        .await
        .expect("connect");
    let code = running
        .passkeys
        .find_allowed(&revoked)
        .await
        .expect("found")
        .expect("allowed")
        .code;

    running
        .passkeys
        .remove_allowed(&code)
        .await
        .expect("revoked");
    advance(Duration::from_secs(60)).await;
    resume();

    assert_eq!(close_code(&mut revoked_ws).await, CloseCode::Policy);
    assert_eq!(acked(&mut kept_ws).await, WsMessage::Text("ack".into()));
}

#[tokio::test(start_paused = true)]
async fn a_failed_recheck_closes_the_socket() {
    let running = spawn_app().await;
    let (_, cookie) = signed_in(&running.app).await;
    let mut ws = connect(running.addr, Some(&cookie)).await.expect("connect");

    running.passkeys.lookups_fail.store(true, Ordering::SeqCst);
    advance(Duration::from_secs(60)).await;
    resume();

    assert_eq!(close_code(&mut ws).await, CloseCode::Error);
}
