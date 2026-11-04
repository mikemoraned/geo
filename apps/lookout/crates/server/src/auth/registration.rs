use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{CreationChallengeResponse, RegisterPublicKeyCredential};
use webauthn_rs_proto::ResidentKeyRequirement;

use crate::auth::code::ShortCode;
use crate::auth::store::{Ceremony, CeremonyId, PendingPasskey};
use crate::auth::{ACCOUNT, Allowing, AuthError, Finish, RelyingParty, Started, account_handle};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Registered {
    Pending { code: ShortCode },
    Allowed,
}

pub async fn start(
    State(relying_party): State<RelyingParty>,
) -> Result<Json<Started<CreationChallengeResponse>>, AuthError> {
    let (mut options, registration) = relying_party
        .webauthn
        .start_passkey_registration(account_handle(), ACCOUNT, ACCOUNT, None)
        .map_err(AuthError::Unstarted)?;
    require_discoverable(&mut options);
    let ceremony = CeremonyId::random();
    relying_party
        .store
        .put_ceremony(ceremony, Ceremony::Registration(registration))
        .await?;
    Ok(Json(Started { ceremony, options }))
}

pub async fn finish(
    State(relying_party): State<RelyingParty>,
    Json(finish): Json<Finish<RegisterPublicKeyCredential>>,
) -> Result<Json<Registered>, AuthError> {
    let Some(Ceremony::Registration(registration)) =
        relying_party.store.take_ceremony(finish.ceremony).await?
    else {
        return Err(AuthError::NoCeremony);
    };
    let passkey = relying_party
        .webauthn
        .finish_passkey_registration(&finish.credential, &registration)
        .map_err(AuthError::Unverified)?;
    let pending = PendingPasskey {
        passkey,
        registered_at: Utc::now(),
    };
    let code = relying_party.store.put_pending(pending).await?;
    match relying_party.allowing {
        Allowing::ByAdmin => {
            tracing::info!(%code, "registered a passkey to wait for the admin");
            Ok(Json(Registered::Pending { code }))
        }
        Allowing::AtOnce => {
            relying_party.store.allow(&code, None).await?;
            tracing::info!(%code, "registered a passkey and allowed it at once");
            Ok(Json(Registered::Allowed))
        }
    }
}

fn require_discoverable(options: &mut CreationChallengeResponse) {
    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
}
