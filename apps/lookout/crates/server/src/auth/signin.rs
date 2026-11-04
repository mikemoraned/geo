use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use tower_sessions::Session;
use webauthn_rs::prelude::{
    CredentialID, DiscoverableKey, PublicKeyCredential, RequestChallengeResponse,
};

use crate::auth::sessions::SIGNED_IN_PASSKEY;
use crate::auth::store::{Ceremony, CeremonyId, Update};
use crate::auth::{AuthError, Finish, RelyingParty, Started, account_handle};

pub async fn start(
    State(relying_party): State<RelyingParty>,
) -> Result<Json<Started<RequestChallengeResponse>>, AuthError> {
    let (mut options, authentication) = relying_party
        .webauthn
        .start_discoverable_authentication()
        .map_err(AuthError::Unstarted)?;
    options.mediation = None;
    let ceremony = CeremonyId::random();
    relying_party
        .store
        .put_ceremony(ceremony, Ceremony::SignIn(authentication))
        .await?;
    Ok(Json(Started { ceremony, options }))
}

pub async fn finish(
    State(relying_party): State<RelyingParty>,
    session: Session,
    Json(finish): Json<Finish<PublicKeyCredential>>,
) -> Result<StatusCode, AuthError> {
    let Some(Ceremony::SignIn(authentication)) =
        relying_party.store.take_ceremony(finish.ceremony).await?
    else {
        return Err(AuthError::NoCeremony);
    };
    let (handle, id) = relying_party
        .webauthn
        .identify_discoverable_authentication(&finish.credential)
        .map_err(AuthError::Unverified)?;
    if handle != account_handle() {
        return Err(AuthError::NotAllowed);
    }
    let id = CredentialID::from(id.to_vec());
    let Some(mut allowed) = relying_party.store.find_allowed(&id).await? else {
        return Err(AuthError::NotAllowed);
    };
    let result = relying_party
        .webauthn
        .finish_discoverable_authentication(
            &finish.credential,
            authentication,
            &[DiscoverableKey::from(&allowed.passkey)],
        )
        .map_err(AuthError::Unverified)?;
    allowed.passkey.update_credential(&result);
    allowed.last_used_at = Some(Utc::now());
    let code = allowed.code.clone();
    if relying_party.store.update_allowed(allowed).await? == Update::Gone {
        return Err(AuthError::NotAllowed);
    }
    session.cycle_id().await?;
    session.insert(SIGNED_IN_PASSKEY, &id).await?;
    tracing::info!(%code, "signed in with a passkey");
    Ok(StatusCode::NO_CONTENT)
}
