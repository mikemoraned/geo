#![cfg(test)]

use webauthn_rs::prelude::{Passkey, Url, Uuid, Webauthn, WebauthnBuilder};

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

pub fn registration_ceremony() -> Ceremony {
    let (_, registration) = webauthn()
        .start_passkey_registration(Uuid::new_v4(), "recording", "recording", None)
        .expect("a registration");
    Ceremony::Registration(registration)
}

pub async fn registered_passkey() -> Passkey {
    let (passkey, _) = test_passkeys::registered_passkey(&origin(), "recording").await;
    passkey
}
