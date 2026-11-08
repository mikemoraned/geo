use async_trait::async_trait;
use passkey::authenticator::{
    Authenticator, CredentialStore, UiHint, UserCheck, UserValidationMethod,
};
use passkey::crypto::AvailableBackend;
use passkey::types::Passkey;
use passkey::types::ctap2::{Aaguid, Ctap2Error};

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
