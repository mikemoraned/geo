use axum::extract::{FromRef, FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use tower_sessions::Session;
use tower_sessions::session::Id;

use crate::auth::RelyingParty;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedIn {
    pub session: Id,
}

impl<S> OptionalFromRequestParts<S> for SignedIn
where
    RelyingParty: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let signed_in = RelyingParty::from_ref(state)
            .signed_in(&session)
            .await
            .map_err(IntoResponse::into_response)?;
        if !signed_in {
            return Ok(None);
        }
        Ok(session.id().map(|session| SignedIn { session }))
    }
}
