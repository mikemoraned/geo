use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use tower_sessions::Session;

use crate::auth::{AuthError, RelyingParty};

const SIGNIN_PAGE: &str = "/signin/";

pub async fn require_signin(
    State(relying_party): State<RelyingParty>,
    session: Session,
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    Ok(if relying_party.signed_in(&session).await? {
        next.run(request).await
    } else {
        Redirect::to(SIGNIN_PAGE).into_response()
    })
}
