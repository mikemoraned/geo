use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use passkey::types::webauthn::{CredentialCreationOptions, ResidentKeyRequirement};
use server::auth::redis::RedisAuthStore;
use server::auth::registration::Registered;
use server::auth::sessions::SharedSessionStore;
use server::auth::store::{AuthStore, CeremonyId, PendingPasskey};
use server::auth::{Allowing, AuthError, Finish, RelyingParty, Started};
use std::str::FromStr;
use test_passkeys::registered_passkey;
use test_redis::TestRedis;

use tower_sessions::cookie::SameSite;
use tower_sessions::cookie::time::Duration;
use tower_sessions::session::Id;
use tower_sessions::{MemoryStore, SessionStore};

use crate::support::{
    Browser, LOCALHOST, Lookup, ORIGIN, ScriptedAuthStore, Server, app, finish_registration,
    finish_signin, get, post, register, sign_in, start_registration, start_signin, url,
};

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
async fn a_credential_that_fails_verification_is_refused_without_the_reason() {
    let server = Server::hosted(Allowing::ByAdmin);
    let challenger = start_registration(&server.app).await;
    let other = start_registration(&server.app).await;
    let misdirected = Finish {
        ceremony: other.ceremony,
        credential: Browser::at(ORIGIN).register(challenger.options).await,
    };
    let json = serde_json::to_vec(&misdirected).expect("a finish");

    let refused = post(&server.app, "/auth/register/finish", Body::from(json), None).await;

    assert_eq!(
        (refused.status, refused.body),
        (
            StatusCode::BAD_REQUEST,
            Bytes::from("the credential failed verification")
        )
    );
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
    let store = Arc::new(ScriptedAuthStore::new(Lookup::RevokeOnceFound));
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
    let (passkey, held) = registered_passkey(&url(ORIGIN), "another").await;
    let mut browser = Browser::holding(ORIGIN, held);
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
async fn each_request_renews_the_session() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let cookie = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    let id = Id::from_str(cookie.value()).expect("a session id");
    let expiry = async || {
        server
            .sessions
            .load(&id)
            .await
            .expect("loaded")
            .expect("a session")
            .expiry_date
    };
    let signed_in_until = expiry().await;

    get(&server.app, "/auth/status", Some(&cookie)).await;

    let renewed_until = expiry().await;
    assert!(
        renewed_until > signed_in_until,
        "{renewed_until} is not after {signed_in_until}"
    );
}

const UNGATED_PATHS: [&str; 3] = ["/index.html", "/signin/", "/version"];

#[tokio::test]
async fn a_request_outside_the_gate_leaves_the_session_alone() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let cookie = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    let id = Id::from_str(cookie.value()).expect("a session id");
    let stored = async || server.sessions.load(&id).await.expect("loaded");
    let signed_in = stored().await;

    for path in UNGATED_PATHS {
        let reply = get(&server.app, path, Some(&cookie)).await;

        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(
            reply.session_cookie().is_none(),
            "{path}: {:?}",
            reply.headers
        );
        assert_eq!(stored().await, signed_in, "{path}");
    }
}

#[tokio::test]
async fn a_stale_cookie_outside_the_gate_starts_no_session() {
    let server = Server::hosted(Allowing::AtOnce);
    let mut browser = Browser::at(ORIGIN);
    register(&server.app, &mut browser).await;
    let stale = sign_in(&server.app, &mut browser, None)
        .await
        .session_cookie()
        .expect("a session");
    post(&server.app, "/auth/signout", Body::empty(), Some(&stale)).await;

    for path in UNGATED_PATHS {
        let reply = get(&server.app, path, Some(&stale)).await;

        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(
            reply.session_cookie().is_none(),
            "{path}: {:?}",
            reply.headers
        );
    }
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
