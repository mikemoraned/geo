# test-passkeys

Builds a software authenticator for a test. Tests that register or sign in a passkey take this
crate as a dev-dependency, so the unit tests and the integration tests share one authenticator.

`approving_authenticator` takes the store that holds the authenticator's passkeys. The
authenticator passes every user-presence and user-verification check it is asked to make. It comes
from [1Password's `passkey` crate][passkey], which supports discoverable passkeys and user handles.

`registered_passkey` registers a passkey for a user on an origin, through webauthn-rs and that
authenticator. It returns two halves. The server's [`Passkey`][webauthn-passkey] holds the public
key, as a store keeps it. The authenticator's passkey holds the private key, so a test can sign in
with it later.

[passkey]: https://docs.rs/passkey/0.6.0/passkey/
[webauthn-passkey]: https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/prelude/struct.Passkey.html
