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
    CredentialRequestOptions,
};
use server::auth::code::ShortCode;
use server::auth::memory::MemoryAuthStore;
use server::auth::registration::Registered;
use server::auth::sessions::SharedSessionStore;
use server::auth::store::AuthStore;
use server::auth::{Allowing, Finish, RelyingParty, Started};
use server::{AppState, build_app};
use tower::ServiceExt;

use tower_sessions::MemoryStore;
use tower_sessions::cookie::Cookie;
use webauthn_rs::prelude::{CredentialID, Url};

pub const ORIGIN: &str = "https://lookout.example.com";
pub const LOCALHOST: &str = "http://localhost:3000";

pub const RECORD_PATHS: [&str; 4] = [
    "/record",
    "/record/",
    "/record/index.html",
    "/record/app.js",
];

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

    pub async fn register(
        &mut self,
        options: CredentialCreationOptions,
    ) -> CreatedPublicKeyCredential {
        let mut client = Client::new(self.authenticator()).allows_insecure_localhost(true);
        let created = client
            .register(&self.origin, options, DefaultClientData)
            .await
            .expect("a credential");
        self.passkey = client.authenticator().store().clone();
        created
    }

    pub async fn authenticate(
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

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
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

pub fn url(origin: &str) -> Url {
    Url::parse(origin).expect("an origin")
}

pub fn app(relying_party: RelyingParty) -> Router {
    build_app(
        AppState {
            sink: None,
            relying_party,
        },
        env!("CARGO_MANIFEST_DIR"),
    )
}

pub struct Server {
    pub app: Router,
    pub passkeys: Arc<MemoryAuthStore>,
    pub sessions: MemoryStore,
}

impl Server {
    pub fn hosted(allowing: Allowing) -> Self {
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

pub fn with_cookie(request: request::Builder, cookie: Option<&Cookie<'_>>) -> request::Builder {
    match cookie {
        Some(cookie) => request.header(header::COOKIE, cookie.stripped().to_string()),
        None => request,
    }
}

pub async fn send(app: &Router, request: Request<Body>) -> Reply {
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

pub async fn get(app: &Router, path: &str, cookie: Option<&Cookie<'_>>) -> Reply {
    let request = with_cookie(Request::get(path), cookie);
    send(app, request.body(Body::empty()).expect("a request")).await
}

pub async fn start_registration(app: &Router) -> Started<CredentialCreationOptions> {
    let reply = post(app, "/auth/register/start", Body::empty(), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    serde_json::from_slice(&reply.body).expect("a started registration")
}

pub async fn finish_registration(
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

pub async fn start_signin(app: &Router) -> (Started<CredentialRequestOptions>, serde_json::Value) {
    let reply = post(app, "/auth/signin/start", Body::empty(), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let started = serde_json::from_slice(&reply.body).expect("a started sign-in");
    let json = serde_json::from_slice(&reply.body).expect("json");
    (started, json)
}

pub async fn finish_signin(
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

pub async fn signed_in_with_code(server: &Server) -> (ShortCode, Cookie<'static>) {
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
