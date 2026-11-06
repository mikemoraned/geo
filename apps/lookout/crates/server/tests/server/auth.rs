use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode, header, request};
use passkey::authenticator::{Authenticator, UiHint, UserCheck, UserValidationMethod};
use passkey::client::{Client, DefaultClientData};
use passkey::crypto::AvailableBackend;
use passkey::types::Passkey;
use passkey::types::ctap2::{Aaguid, Ctap2Error};
use passkey::types::webauthn::{
    AuthenticatedPublicKeyCredential, CreatedPublicKeyCredential, CredentialCreationOptions,
    CredentialRequestOptions, ResidentKeyRequirement,
};
use server::auth::code::ShortCode;
use server::auth::memory::MemoryAuthStore;
use server::auth::redis::RedisAuthStore;
use server::auth::registration::Registered;
use server::auth::sessions::SharedSessionStore;
use server::auth::status::SignInStatus;
use server::auth::store::{
    AllowedPasskey, AuthStore, Ceremony, CeremonyId, Insertion, PendingPasskey, StoreError, Update,
};
use server::auth::{Allowing, AuthError, Finish, RelyingParty, Started};
use server::{AppState, build_app};
use std::str::FromStr;
use test_redis::TestRedis;
use tower::ServiceExt;

use tower_sessions::cookie::time::Duration;
use tower_sessions::cookie::{Cookie, SameSite};
use tower_sessions::session::Id;
use tower_sessions::{MemoryStore, SessionStore};
use webauthn_rs::prelude::{CredentialID, RegisterPublicKeyCredential, Url, Uuid, WebauthnBuilder};

const ORIGIN: &str = "https://lookout.example.com";
pub const LOCALHOST: &str = "http://localhost:3000";

struct Approving;

#[async_trait]
impl UserValidationMethod for Approving {
    type PasskeyItem = Passkey;

    async fn check_user<'a>(
        &self,
        _hint: UiHint<'a, Self::PasskeyItem>,
        presence: bool,
        verification: bool,
    ) -> Result<UserCheck, Ctap2Error> {
        Ok(UserCheck {
            presence,
            verification,
        })
    }

    fn is_presence_enabled(&self) -> bool {
        true
    }

    fn is_verification_enabled(&self) -> Option<bool> {
        Some(true)
    }
}

pub struct Browser {
    origin: Url,
    passkey: Option<Passkey>,
}

impl Browser {
    pub fn at(origin: &str) -> Self {
        Self {
            origin: url(origin),
            passkey: None,
        }
    }

    fn authenticator(&self) -> Authenticator<Option<Passkey>, Approving, AvailableBackend> {
        Authenticator::new(
            Aaguid::new_empty(),
            self.passkey.clone(),
            Approving,
            AvailableBackend,
        )
    }

    async fn register(&mut self, options: CredentialCreationOptions) -> CreatedPublicKeyCredential {
        let mut client = Client::new(self.authenticator()).allows_insecure_localhost(true);
        let created = client
            .register(&self.origin, options, DefaultClientData)
            .await
            .expect("a credential");
        self.passkey = client.authenticator().store().clone();
        created
    }

    async fn authenticate(
        &mut self,
        options: CredentialRequestOptions,
    ) -> AuthenticatedPublicKeyCredential {
        let mut client = Client::new(self.authenticator()).allows_insecure_localhost(true);
        let asserted = client
            .authenticate(&self.origin, options, DefaultClientData)
            .await
            .expect("an assertion");
        self.passkey = client.authenticator().store().clone();
        asserted
    }
}

#[derive(Default)]
struct RevokedOnceFound {
    inner: MemoryAuthStore,
}

#[async_trait]
impl AuthStore for RevokedOnceFound {
    async fn put_ceremony(&self, id: CeremonyId, ceremony: Ceremony) -> Result<(), StoreError> {
        self.inner.put_ceremony(id, ceremony).await
    }

    async fn take_ceremony(&self, id: CeremonyId) -> Result<Option<Ceremony>, StoreError> {
        self.inner.take_ceremony(id).await
    }

    async fn insert_pending(
        &self,
        code: &ShortCode,
        passkey: PendingPasskey,
    ) -> Result<Insertion, StoreError> {
        self.inner.insert_pending(code, passkey).await
    }

    async fn take_pending(&self, code: &ShortCode) -> Result<Option<PendingPasskey>, StoreError> {
        self.inner.take_pending(code).await
    }

    async fn find_allowed(&self, id: &CredentialID) -> Result<Option<AllowedPasskey>, StoreError> {
        let found = self.inner.find_allowed(id).await?;
        if let Some(allowed) = &found {
            self.inner.remove_allowed(&allowed.code).await?;
        }
        Ok(found)
    }

    async fn put_allowed(&self, passkey: AllowedPasskey) -> Result<(), StoreError> {
        self.inner.put_allowed(passkey).await
    }

    async fn update_allowed(&self, passkey: AllowedPasskey) -> Result<Update, StoreError> {
        self.inner.update_allowed(passkey).await
    }

    async fn list_allowed(&self) -> Result<Vec<AllowedPasskey>, StoreError> {
        self.inner.list_allowed().await
    }

    async fn remove_allowed(&self, code: &ShortCode) -> Result<Option<AllowedPasskey>, StoreError> {
        self.inner.remove_allowed(code).await
    }
}

pub struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Reply {
    pub fn session_cookie(&self) -> Option<Cookie<'static>> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| {
                Cookie::parse(value.to_str().expect("a header").to_string()).expect("a cookie")
            })
            .find(|cookie| cookie.name().contains("lookout-session"))
    }
}

fn url(origin: &str) -> Url {
    Url::parse(origin).expect("an origin")
}

pub fn static_dir() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string()
}

fn app(relying_party: RelyingParty) -> Router {
    build_app(
        AppState {
            sink: None,
            relying_party,
        },
        static_dir(),
    )
}

struct Server {
    app: Router,
    passkeys: Arc<MemoryAuthStore>,
    sessions: MemoryStore,
}

impl Server {
    fn hosted(allowing: Allowing) -> Self {
        let passkeys = Arc::new(MemoryAuthStore::default());
        let sessions = MemoryStore::default();
        let relying_party = RelyingParty::new(
            &url(ORIGIN),
            passkeys.clone(),
            SharedSessionStore::new(sessions.clone()),
            allowing,
        )
        .expect("a relying party");
        Self {
            app: app(relying_party),
            passkeys,
            sessions,
        }
    }
}

fn with_cookie(request: request::Builder, cookie: Option<&Cookie<'_>>) -> request::Builder {
    match cookie {
        Some(cookie) => request.header(header::COOKIE, cookie.stripped().to_string()),
        None => request,
    }
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body");
    Reply {
        status,
        headers,
        body,
    }
}

pub async fn post(app: &Router, path: &str, body: Body, cookie: Option<&Cookie<'_>>) -> Reply {
    let request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
    send(
        app,
        with_cookie(request, cookie).body(body).expect("a request"),
    )
    .await
}

async fn get(app: &Router, path: &str, cookie: Option<&Cookie<'_>>) -> Reply {
    let request = with_cookie(Request::get(path), cookie);
    send(app, request.body(Body::empty()).expect("a request")).await
}

async fn start_registration(app: &Router) -> Started<CredentialCreationOptions> {
    let reply = post(app, "/auth/register/start", Body::empty(), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    serde_json::from_slice(&reply.body).expect("a started registration")
}

async fn finish_registration(
    app: &Router,
    finish: &Finish<CreatedPublicKeyCredential>,
) -> Result<Registered, StatusCode> {
    let json = serde_json::to_vec(finish).expect("a finish");
    let reply = post(app, "/auth/register/finish", Body::from(json), None).await;
    match reply.status {
        StatusCode::OK => Ok(serde_json::from_slice(&reply.body).expect("a registration")),
        refused => Err(refused),
    }
}

pub async fn register(app: &Router, browser: &mut Browser) -> (CredentialID, Registered) {
    let started = start_registration(app).await;
    let credential = browser.register(started.options).await;
    let id = CredentialID::from(credential.raw_id.to_vec());
    let finish = Finish {
        ceremony: started.ceremony,
        credential,
    };
    let registered = finish_registration(app, &finish)
        .await
        .expect("a registration");
    (id, registered)
}

async fn start_signin(app: &Router) -> (Started<CredentialRequestOptions>, serde_json::Value) {
    let reply = post(app, "/auth/signin/start", Body::empty(), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let started = serde_json::from_slice(&reply.body).expect("a started sign-in");
    let json = serde_json::from_slice(&reply.body).expect("json");
    (started, json)
}

async fn finish_signin(
    app: &Router,
    finish: &Finish<AuthenticatedPublicKeyCredential>,
    cookie: Option<&Cookie<'_>>,
) -> Reply {
    let json = serde_json::to_vec(finish).expect("a finish");
    post(app, "/auth/signin/finish", Body::from(json), cookie).await
}

pub async fn sign_in(app: &Router, browser: &mut Browser, cookie: Option<&Cookie<'_>>) -> Reply {
    let (started, _) = start_signin(app).await;
    let credential = browser.authenticate(started.options).await;
    let finish = Finish {
        ceremony: started.ceremony,
        credential,
    };
    finish_signin(app, &finish, cookie).await
}

#[tokio::test]
async fn a_registration_waits_under_its_code_for_the_admin() {
    let server = Server::hosted(Allowing::ByAdmin);

    let (id, registered) = register(&server.app, &mut Browser::at(ORIGIN)).await;

    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };
    assert!(
        server
            .passkeys
            .find_allowed(&id)
            .await
            .expect("found")
            .is_none()
    );
    let allowed = server
        .passkeys
        .allow(&code, None)
        .await
        .expect("allowed")
        .expect("a pending passkey");
    assert_eq!(allowed.passkey.cred_id(), &id);
}

#[tokio::test]
async fn a_registration_allowed_at_once_needs_no_admin() {
    let server = Server::hosted(Allowing::AtOnce);

    let (id, registered) = register(&server.app, &mut Browser::at(ORIGIN)).await;

    assert_eq!(registered, Registered::Allowed);
    assert!(
        server
            .passkeys
            .find_allowed(&id)
            .await
            .expect("found")
            .is_some()
    );
}

#[tokio::test]
async fn in_memory_only_a_passkey_registered_for_localhost_is_allowed_at_once() {
    let local = app(RelyingParty::in_memory(&url(LOCALHOST)).expect("a relying party"));
    let hosted = app(RelyingParty::in_memory(&url(ORIGIN)).expect("a relying party"));

    let (_, at_localhost) = register(&local, &mut Browser::at(LOCALHOST)).await;
    let (_, elsewhere) = register(&hosted, &mut Browser::at(ORIGIN)).await;

    assert_eq!(at_localhost, Registered::Allowed);
    assert!(
        matches!(elsewhere, Registered::Pending { .. }),
        "{elsewhere:?} is not pending"
    );
}

#[tokio::test]
async fn over_redis_even_a_localhost_passkey_waits_for_the_admin_docker() {
    let redis = TestRedis::start().await;
    let app = app(
        RelyingParty::over_redis(&url(LOCALHOST), redis.connection()).expect("a relying party"),
    );

    let (_, registered) = register(&app, &mut Browser::at(LOCALHOST)).await;

    assert!(
        matches!(registered, Registered::Pending { .. }),
        "{registered:?} is not pending"
    );
}

#[tokio::test]
async fn over_redis_a_signin_keeps_its_session_in_redis_docker() {
    let redis = TestRedis::start().await;
    let app =
        app(RelyingParty::over_redis(&url(ORIGIN), redis.connection()).expect("a relying party"));
    let mut browser = Browser::at(ORIGIN);
    let (_, registered) = register(&app, &mut browser).await;
    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };
    RedisAuthStore::new(redis.connection())
        .allow(&code, None)
        .await
        .expect("allowed")
        .expect("a pending passkey");

    let reply = sign_in(&app, &mut browser, None).await;

    let cookie = reply.session_cookie().expect("a session cookie");
    let ttl = redis
        .seconds_to_live(&format!("lookout-session:{}", cookie.value()))
        .await;
    assert!(ttl > 0, "{ttl}");
}

#[tokio::test]
async fn registration_asks_for_a_discoverable_passkey() {
    let server = Server::hosted(Allowing::ByAdmin);

    let options = start_registration(&server.app).await.options.public_key;

    let selection = options
        .authenticator_selection
        .expect("authenticator selection");
    assert_eq!(
        selection.resident_key,
        Some(ResidentKeyRequirement::Required)
    );
}

#[tokio::test]
async fn every_registration_is_for_the_recording_account() {
    let server = Server::hosted(Allowing::ByAdmin);

    let first = start_registration(&server.app)
        .await
        .options
        .public_key
        .user;
    let second = start_registration(&server.app)
        .await
        .options
        .public_key
        .user;

    assert_eq!(first.name, "recording");
    assert_eq!(first.id, second.id);
}

#[tokio::test]
async fn registration_names_no_passkey_the_server_holds() {
    let server = Server::hosted(Allowing::AtOnce);
    register(&server.app, &mut Browser::at(ORIGIN)).await;

    let options = start_registration(&server.app).await.options.public_key;

    assert!(options.exclude_credentials.is_none());
}

#[tokio::test]
async fn a_ceremony_finishes_once() {
    let server = Server::hosted(Allowing::ByAdmin);
    let started = start_registration(&server.app).await;
    let credential = Browser::at(ORIGIN).register(started.options).await;
    let finish = Finish {
        ceremony: started.ceremony,
        credential,
    };

    let first = finish_registration(&server.app, &finish).await;
    let replayed = finish_registration(&server.app, &finish).await;

    assert!(first.is_ok(), "{first:?}");
    assert_eq!(replayed, Err(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn a_finish_needs_a_ceremony_the_server_started() {
    let server = Server::hosted(Allowing::ByAdmin);
    let started = start_registration(&server.app).await;
    let credential = Browser::at(ORIGIN).register(started.options).await;
    let unknown = Finish {
        ceremony: CeremonyId::random(),
        credential,
    };

    let finished = finish_registration(&server.app, &unknown).await;

    assert_eq!(finished, Err(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn a_credential_answers_only_the_ceremony_that_challenged_it() {
    let server = Server::hosted(Allowing::ByAdmin);
    let challenger = start_registration(&server.app).await;
    let other = start_registration(&server.app).await;
    let credential = Browser::at(ORIGIN).register(challenger.options).await;
    let misdirected = Finish {
        ceremony: other.ceremony,
        credential,
    };

    let refused = finish_registration(&server.app, &misdirected).await;
    let answered = Finish {
        ceremony: challenger.ceremony,
        credential: misdirected.credential,
    };
    let accepted = finish_registration(&server.app, &answered).await;

    assert_eq!(refused, Err(StatusCode::BAD_REQUEST));
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[tokio::test]
async fn an_allowed_passkey_signs_in_to_a_session() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;

    let reply = sign_in(&server.app, &mut browser, None).await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert!(reply.session_cookie().is_some(), "{:?}", reply.headers);
}

#[tokio::test]
async fn a_pending_passkey_signs_in_once_the_admin_allows_it() {
    let server = Server::hosted(Allowing::ByAdmin);
    let mut browser = Browser::at(ORIGIN);
    let (_, registered) = register(&server.app, &mut browser).await;
    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };

    let waiting = sign_in(&server.app, &mut browser, None).await;
    server.passkeys.allow(&code, None).await.expect("allowed");
    let allowed = sign_in(&server.app, &mut browser, None).await;

    assert_eq!(waiting.status, StatusCode::FORBIDDEN);
    assert!(waiting.session_cookie().is_none(), "{:?}", waiting.headers);
    assert_eq!(allowed.status, StatusCode::NO_CONTENT, "{:?}", allowed.body);
}

#[tokio::test]
async fn a_refusal_answers_with_its_reason_alone() {
    let server = Server::hosted(Allowing::ByAdmin);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let (started, _) = start_signin(&server.app).await;
    let finish = Finish {
        ceremony: started.ceremony,
        credential: browser.authenticate(started.options).await,
    };

    let not_allowed = finish_signin(&server.app, &finish, None).await;
    let replayed = finish_signin(&server.app, &finish, None).await;

    assert_eq!(
        (not_allowed.status, not_allowed.body),
        (
            StatusCode::FORBIDDEN,
            Bytes::from(AuthError::NotAllowed.to_string())
        )
    );
    assert_eq!(
        (replayed.status, replayed.body),
        (
            StatusCode::BAD_REQUEST,
            Bytes::from(AuthError::NoCeremony.to_string())
        )
    );
}

#[tokio::test]
async fn a_revoked_passkey_cannot_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let mut browser = Browser::at(ORIGIN);
    let (_, registered) = register(&server.app, &mut browser).await;
    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };
    server.passkeys.allow(&code, None).await.expect("allowed");
    server
        .passkeys
        .remove_allowed(&code)
        .await
        .expect("revoked");

    let reply = sign_in(&server.app, &mut browser, None).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(reply.session_cookie().is_none(), "{:?}", reply.headers);
}

#[tokio::test]
async fn a_passkey_revoked_during_its_signin_stays_revoked() {
    let store = Arc::new(RevokedOnceFound::default());
    let sessions = SharedSessionStore::new(MemoryStore::default());
    let relying_party = RelyingParty::new(&url(ORIGIN), store.clone(), sessions, Allowing::AtOnce)
        .expect("a relying party");
    let app = app(relying_party);
    let mut browser = Browser::at(ORIGIN);
    let (id, _) = register(&app, &mut browser).await;

    let reply = sign_in(&app, &mut browser, None).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(reply.session_cookie().is_none(), "{:?}", reply.headers);
    assert!(
        store
            .inner
            .find_allowed(&id)
            .await
            .expect("found")
            .is_none()
    );
}

#[tokio::test]
async fn a_passkey_for_another_account_cannot_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let mut browser = Browser::at(ORIGIN);
    let webauthn = WebauthnBuilder::new("lookout.example.com", &url(ORIGIN))
        .and_then(|builder| builder.build())
        .expect("a relying party");
    let (options, registration) = webauthn
        .start_passkey_registration(Uuid::new_v4(), "another", "another", None)
        .expect("a registration");
    let created = browser
        .register(
            serde_json::from_value(serde_json::to_value(options).expect("json")).expect("options"),
        )
        .await;
    let credential: RegisterPublicKeyCredential =
        serde_json::from_value(serde_json::to_value(created).expect("json")).expect("a credential");
    let passkey = webauthn
        .finish_passkey_registration(&credential, &registration)
        .expect("a passkey");
    let code = server
        .passkeys
        .put_pending(PendingPasskey {
            passkey,
            registered_at: chrono::Utc::now(),
        })
        .await
        .expect("stored");
    server.passkeys.allow(&code, None).await.expect("allowed");

    let reply = sign_in(&server.app, &mut browser, None).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(reply.session_cookie().is_none(), "{:?}", reply.headers);
}

#[tokio::test]
async fn sign_in_names_no_passkey_and_asks_for_the_usual_prompt() {
    let server = Server::hosted(Allowing::AtOnce);
    register(&server.app, &mut Browser::at(ORIGIN)).await;

    let (_, json) = start_signin(&server.app).await;

    let allowed = &json["options"]["publicKey"]["allowCredentials"];
    assert!(allowed.as_array().is_none_or(Vec::is_empty), "{allowed}");
    assert!(json["options"].get("mediation").is_none(), "{json}");
}

#[tokio::test]
async fn a_signin_ceremony_finishes_once() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let (started, _) = start_signin(&server.app).await;
    let credential = browser.authenticate(started.options).await;
    let finish = Finish {
        ceremony: started.ceremony,
        credential,
    };

    let first = finish_signin(&server.app, &finish, None).await;
    let replayed = finish_signin(&server.app, &finish, None).await;

    assert_eq!(first.status, StatusCode::NO_CONTENT, "{:?}", first.body);
    assert_eq!(replayed.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn signing_in_records_when_the_passkey_was_used() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    let (id, _) = register(&server.app, &mut browser).await;

    sign_in(&server.app, &mut browser, None).await;

    let allowed = server
        .passkeys
        .find_allowed(&id)
        .await
        .expect("found")
        .expect("allowed");
    assert!(allowed.last_used_at.is_some());
}

#[tokio::test]
async fn signing_in_again_issues_a_fresh_session_id() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;

    let first = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    let second = sign_in(&server.app, &mut browser, Some(&first))
        .await
        .session_cookie()
        .expect("a session");

    assert_ne!(first.value(), second.value());
}

#[tokio::test]
async fn a_hosted_session_cookie_is_host_only_and_strict() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;

    let cookie = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");

    assert_eq!(cookie.name(), "__Host-lookout-session");
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.domain(), None);
}

#[tokio::test]
async fn a_localhost_session_cookie_drops_the_secure_prefix() {
    let app = app(RelyingParty::in_memory(&url(LOCALHOST)).expect("a relying party"));
    let mut browser = Browser::at(LOCALHOST);
    register(&app, &mut browser).await;

    let cookie = sign_in(&app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");

    assert_eq!(cookie.name(), "lookout-session");
    assert_ne!(cookie.secure(), Some(true));
}

#[tokio::test]
async fn registration_creates_no_session() {
    let server = Server::hosted(Allowing::AtOnce);
    let started = post(&server.app, "/auth/register/start", Body::empty(), None).await;

    let mut browser = Browser::at(ORIGIN);
    let options: Started<CredentialCreationOptions> =
        serde_json::from_slice(&started.body).expect("a started registration");
    let credential = browser.register(options.options).await;
    let finish = Finish {
        ceremony: options.ceremony,
        credential,
    };
    let finished = post(
        &server.app,
        "/auth/register/finish",
        Body::from(serde_json::to_vec(&finish).expect("json")),
        None,
    )
    .await;

    assert!(started.session_cookie().is_none(), "{:?}", started.headers);
    assert!(
        finished.session_cookie().is_none(),
        "{:?}",
        finished.headers
    );
}

#[tokio::test]
async fn signing_out_ends_the_session() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let signed_in = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    let id = Id::from_str(signed_in.value()).expect("a session id");
    assert!(server.sessions.load(&id).await.expect("loaded").is_some());

    let reply = post(
        &server.app,
        "/auth/signout",
        Body::empty(),
        Some(&signed_in),
    )
    .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert!(server.sessions.load(&id).await.expect("loaded").is_none());
    let cleared = reply.session_cookie().expect("a cleared cookie");
    assert_eq!(cleared.value(), "");
    assert_eq!(cleared.max_age(), Some(Duration::ZERO));
}

#[tokio::test]
async fn signing_out_without_a_session_changes_nothing() {
    let server = Server::hosted(Allowing::AtOnce);

    let reply = post(&server.app, "/auth/signout", Body::empty(), None).await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    assert!(reply.session_cookie().is_none(), "{:?}", reply.headers);
}

const RECORD_PATHS: [&str; 4] = [
    "/record",
    "/record/",
    "/record/index.html",
    "/record/app.js",
];

fn assert_sent_to_signin(path: &str, reply: &Reply) {
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "{path}: {:?}",
        reply.body
    );
    assert_eq!(
        reply.headers.get(header::LOCATION).map(|to| to.as_bytes()),
        Some(b"/signin/".as_slice()),
        "{path}"
    );
}

async fn signed_in_with_code(server: &Server) -> (ShortCode, Cookie<'static>) {
    let mut browser = Browser::at(ORIGIN);
    let (_, registered) = register(&server.app, &mut browser).await;
    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };
    server.passkeys.allow(&code, None).await.expect("allowed");
    let cookie = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    (code, cookie)
}

#[tokio::test]
async fn the_record_page_sends_a_visitor_without_a_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, None).await);
    }
}

#[tokio::test]
async fn a_signed_in_session_reaches_the_record_page() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    for path in RECORD_PATHS {
        let reply = get(&server.app, path, Some(&cookie)).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn a_browser_asks_the_server_before_reusing_the_record_page() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    for path in RECORD_PATHS {
        let reply = get(&server.app, path, Some(&cookie)).await;
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"private, no-cache"[..]),
            "{path}"
        );
    }
}

#[tokio::test]
async fn revoking_a_passkey_sends_only_its_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (revoked, shut_out) = signed_in_with_code(&server).await;
    let (_, kept) = signed_in_with_code(&server).await;

    server
        .passkeys
        .remove_allowed(&revoked)
        .await
        .expect("revoked");

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, Some(&shut_out)).await);
        let reply = get(&server.app, path, Some(&kept)).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn signing_out_sends_the_session_to_sign_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    post(&server.app, "/auth/signout", Body::empty(), Some(&cookie)).await;

    for path in RECORD_PATHS {
        assert_sent_to_signin(path, &get(&server.app, path, Some(&cookie)).await);
    }
}

#[tokio::test]
async fn the_other_pages_need_no_session() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in ["/", "/live/", "/kiosk/", "/signin/"] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn a_browser_asks_the_server_before_reusing_any_other_file() {
    let server = Server::hosted(Allowing::ByAdmin);

    for path in [
        "/",
        "/signin/",
        "/signin/signin.js",
        "/version.js",
        "/sessions.json",
    ] {
        let reply = get(&server.app, path, None).await;
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .map(|value| value.as_bytes()),
            Some(&b"no-cache"[..]),
            "{path}"
        );
    }
}

async fn sign_in_status(app: &Router, cookie: Option<&Cookie<'_>>) -> SignInStatus {
    let reply = get(app, "/auth/status", cookie).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    serde_json::from_slice(&reply.body).expect("a status")
}

#[tokio::test]
async fn the_status_without_a_session_is_signed_out() {
    let server = Server::hosted(Allowing::ByAdmin);

    assert_eq!(
        sign_in_status(&server.app, None).await,
        SignInStatus::SignedOut
    );
}

#[tokio::test]
async fn the_status_of_a_session_with_an_allowed_passkey_is_signed_in() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    assert_eq!(
        sign_in_status(&server.app, Some(&cookie)).await,
        SignInStatus::SignedIn
    );
}

#[tokio::test]
async fn revoking_a_passkey_signs_out_only_its_session() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (revoked, shut_out) = signed_in_with_code(&server).await;
    let (_, kept) = signed_in_with_code(&server).await;

    server
        .passkeys
        .remove_allowed(&revoked)
        .await
        .expect("revoked");

    assert_eq!(
        (
            sign_in_status(&server.app, Some(&shut_out)).await,
            sign_in_status(&server.app, Some(&kept)).await,
        ),
        (SignInStatus::SignedOut, SignInStatus::SignedIn)
    );
}

#[tokio::test]
async fn the_status_after_signing_out_is_signed_out() {
    let server = Server::hosted(Allowing::ByAdmin);
    let (_, cookie) = signed_in_with_code(&server).await;

    post(&server.app, "/auth/signout", Body::empty(), Some(&cookie)).await;

    assert_eq!(
        sign_in_status(&server.app, Some(&cookie)).await,
        SignInStatus::SignedOut
    );
}
