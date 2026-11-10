use async_trait::async_trait;
use passkey::authenticator::{
    Authenticator, CredentialStore, UiHint, UserCheck, UserValidationMethod,
};
use passkey::client::{Client, DefaultClientData};
use passkey::crypto::AvailableBackend;
use passkey::types::Passkey;
use passkey::types::ctap2::{Aaguid, Ctap2Error};
use serde::Serialize;
use serde::de::DeserializeOwned;
use webauthn_rs::prelude::{RegisterPublicKeyCredential, Url, Uuid, WebauthnBuilder};

pub struct Approving;

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

pub fn approving_authenticator<S: CredentialStore>(
    store: S,
) -> Authenticator<S, Approving, AvailableBackend> {
    Authenticator::new(Aaguid::new_empty(), store, Approving, AvailableBackend)
}

fn through_json<T: DeserializeOwned>(value: &impl Serialize) -> T {
    serde_json::from_value(serde_json::to_value(value).expect("serialise")).expect("deserialise")
}

pub async fn registered_passkey(
    origin: &Url,
    user: &str,
) -> (webauthn_rs::prelude::Passkey, Passkey) {
    let webauthn = WebauthnBuilder::new(origin.domain().expect("a domain"), origin)
        .and_then(|builder| builder.build())
        .expect("a relying party");
    let (options, registration) = webauthn
        .start_passkey_registration(Uuid::new_v4(), user, user, None)
        .expect("a registration");
    let mut client = Client::new(approving_authenticator(None::<Passkey>));
    let created = client
        .register(origin, through_json(&options), DefaultClientData)
        .await
        .expect("a credential");
    let credential: RegisterPublicKeyCredential = through_json(&created);
    let passkey = webauthn
        .finish_passkey_registration(&credential, &registration)
        .expect("a passkey");
    let held = client
        .authenticator()
        .store()
        .clone()
        .expect("a held passkey");
    (passkey, held)
}
