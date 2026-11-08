#![cfg(test)]

use passkey::authenticator::MemoryStore;
use passkey::client::{Client, DefaultClientData};
use serde::Serialize;
use serde::de::DeserializeOwned;
use test_passkeys::approving_authenticator;
use webauthn_rs::prelude::{
    Passkey, RegisterPublicKeyCredential, Url, Uuid, Webauthn, WebauthnBuilder,
};

use crate::auth::store::Ceremony;

const RP_ID: &str = "lookout.example.com";

fn origin() -> Url {
    Url::parse(&format!("https://{RP_ID}")).expect("an origin")
}

fn webauthn() -> Webauthn {
    WebauthnBuilder::new(RP_ID, &origin())
        .and_then(|builder| builder.build())
        .expect("a relying party")
}

fn through_json<T: DeserializeOwned>(value: &impl Serialize) -> T {
    serde_json::from_value(serde_json::to_value(value).expect("serialise")).expect("deserialise")
}

pub fn registration_ceremony() -> Ceremony {
    let (_, registration) = webauthn()
        .start_passkey_registration(Uuid::new_v4(), "recording", "recording", None)
        .expect("a registration");
    Ceremony::Registration(registration)
}

pub async fn registered_passkey() -> Passkey {
    let webauthn = webauthn();
    let (options, registration) = webauthn
        .start_passkey_registration(Uuid::new_v4(), "recording", "recording", None)
        .expect("a registration");
    let authenticator = approving_authenticator(MemoryStore::new());
    let created = Client::new(authenticator)
        .register(&origin(), through_json(&options), DefaultClientData)
        .await
        .expect("a credential");
    let credential: RegisterPublicKeyCredential = through_json(&created);
    webauthn
        .finish_passkey_registration(&credential, &registration)
        .expect("a passkey")
}
