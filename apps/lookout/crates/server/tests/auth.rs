use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use passkey::authenticator::{Authenticator, UiHint, UserCheck, UserValidationMethod};
use passkey::client::{Client, DefaultClientData};
use passkey::crypto::AvailableBackend;
use passkey::types::Passkey;
use passkey::types::ctap2::{Aaguid, Ctap2Error};
use passkey::types::webauthn::{
    CreatedPublicKeyCredential, CredentialCreationOptions, ResidentKeyRequirement,
};
use server::auth::memory::MemoryAuthStore;
use server::auth::registration::Registered;
use server::auth::store::{AuthStore, CeremonyId};
use server::auth::{Allowing, Finish, RelyingParty, Started};
use server::{AppState, build_app};
use tower::ServiceExt;
use webauthn_rs::prelude::{CredentialID, Url};

const ORIGIN: &str = "https://lookout.example.com";
const LOCALHOST: &str = "http://localhost:3000";

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

struct Browser {
    origin: Url,
}

impl Browser {
    fn at(origin: &str) -> Self {
        Self {
            origin: url(origin),
        }
    }

    async fn create(&self, options: CredentialCreationOptions) -> CreatedPublicKeyCredential {
        let authenticator = Authenticator::new(
            Aaguid::new_empty(),
            None::<Passkey>,
            Approving,
            AvailableBackend,
        );
        Client::new(authenticator)
            .allows_insecure_localhost(true)
            .register(&self.origin, options, DefaultClientData)
            .await
            .expect("a credential")
    }
}

fn url(origin: &str) -> Url {
    Url::parse(origin).expect("an origin")
}

fn static_dir() -> String {
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

fn app_over(store: &Arc<MemoryAuthStore>, allowing: Allowing) -> Router {
    let store = Arc::clone(store);
    app(RelyingParty::new(&url(ORIGIN), store, allowing).expect("a relying party"))
}

async fn post(app: &Router, path: &str, body: Body) -> (StatusCode, Bytes) {
    let request = Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body");
    (status, body)
}

async fn start_registration(app: &Router) -> Started<CredentialCreationOptions> {
    let (status, body) = post(app, "/auth/register/start", Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    serde_json::from_slice(&body).expect("a started registration")
}

async fn finish_registration(
    app: &Router,
    finish: &Finish<CreatedPublicKeyCredential>,
) -> Result<Registered, StatusCode> {
    let json = serde_json::to_vec(finish).expect("a finish");
    let (status, body) = post(app, "/auth/register/finish", Body::from(json)).await;
    match status {
        StatusCode::OK => Ok(serde_json::from_slice(&body).expect("a registration")),
        refused => Err(refused),
    }
}

async fn register(app: &Router, browser: &Browser) -> (CredentialID, Registered) {
    let started = start_registration(app).await;
    let credential = browser.create(started.options).await;
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

#[tokio::test]
async fn a_registration_waits_under_its_code_for_the_admin() {
    let store = Arc::new(MemoryAuthStore::default());
    let app = app_over(&store, Allowing::ByAdmin);

    let (id, registered) = register(&app, &Browser::at(ORIGIN)).await;

    let Registered::Pending { code } = registered else {
        panic!("{registered:?} is not pending");
    };
    assert!(store.find_allowed(&id).await.expect("found").is_none());
    let allowed = store
        .allow(&code, None)
        .await
        .expect("allowed")
        .expect("a pending passkey");
    assert_eq!(allowed.passkey.cred_id(), &id);
}

#[tokio::test]
async fn a_registration_allowed_at_once_needs_no_admin() {
    let store = Arc::new(MemoryAuthStore::default());
    let app = app_over(&store, Allowing::AtOnce);

    let (id, registered) = register(&app, &Browser::at(ORIGIN)).await;

    assert_eq!(registered, Registered::Allowed);
    assert!(store.find_allowed(&id).await.expect("found").is_some());
}

#[tokio::test]
async fn in_memory_only_a_passkey_registered_for_localhost_is_allowed_at_once() {
    let local = app(RelyingParty::in_memory(&url(LOCALHOST)).expect("a relying party"));
    let hosted = app(RelyingParty::in_memory(&url(ORIGIN)).expect("a relying party"));

    let (_, at_localhost) = register(&local, &Browser::at(LOCALHOST)).await;
    let (_, elsewhere) = register(&hosted, &Browser::at(ORIGIN)).await;

    assert_eq!(at_localhost, Registered::Allowed);
    assert!(
        matches!(elsewhere, Registered::Pending { .. }),
        "{elsewhere:?} is not pending"
    );
}

#[tokio::test]
async fn registration_asks_for_a_discoverable_passkey() {
    let app = app_over(&Arc::default(), Allowing::ByAdmin);

    let options = start_registration(&app).await.options.public_key;

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
    let app = app_over(&Arc::default(), Allowing::ByAdmin);

    let first = start_registration(&app).await.options.public_key.user;
    let second = start_registration(&app).await.options.public_key.user;

    assert_eq!(first.name, "recording");
    assert_eq!(first.id, second.id);
}

#[tokio::test]
async fn registration_names_no_passkey_the_server_holds() {
    let app = app_over(&Arc::default(), Allowing::AtOnce);
    register(&app, &Browser::at(ORIGIN)).await;

    let options = start_registration(&app).await.options.public_key;

    assert!(options.exclude_credentials.is_none());
}

#[tokio::test]
async fn a_ceremony_finishes_once() {
    let app = app_over(&Arc::default(), Allowing::ByAdmin);
    let started = start_registration(&app).await;
    let credential = Browser::at(ORIGIN).create(started.options).await;
    let finish = Finish {
        ceremony: started.ceremony,
        credential,
    };

    let first = finish_registration(&app, &finish).await;
    let replayed = finish_registration(&app, &finish).await;

    assert!(first.is_ok(), "{first:?}");
    assert_eq!(replayed, Err(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn a_finish_needs_a_ceremony_the_server_started() {
    let app = app_over(&Arc::default(), Allowing::ByAdmin);
    let started = start_registration(&app).await;
    let credential = Browser::at(ORIGIN).create(started.options).await;
    let unknown = Finish {
        ceremony: CeremonyId::random(),
        credential,
    };

    let finished = finish_registration(&app, &unknown).await;

    assert_eq!(finished, Err(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn a_credential_answers_only_the_ceremony_that_challenged_it() {
    let app = app_over(&Arc::default(), Allowing::ByAdmin);
    let challenger = start_registration(&app).await;
    let other = start_registration(&app).await;
    let credential = Browser::at(ORIGIN).create(challenger.options).await;
    let misdirected = Finish {
        ceremony: other.ceremony,
        credential,
    };

    let refused = finish_registration(&app, &misdirected).await;
    let answered = Finish {
        ceremony: challenger.ceremony,
        credential: misdirected.credential,
    };
    let accepted = finish_registration(&app, &answered).await;

    assert_eq!(refused, Err(StatusCode::BAD_REQUEST));
    assert!(accepted.is_ok(), "{accepted:?}");
}
