use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use tower_sessions::Session;

use crate::auth::{AuthError, RelyingParty};

const SIGNIN_PAGE: &str = "/signin/";
const ASK_FIRST: &str = "private, no-cache";

pub async fn require_signin(
    State(relying_party): State<RelyingParty>,
    session: Session,
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    Ok(match relying_party.signed_in(&session).await? {
        Some(_) => {
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(ASK_FIRST));
            response
        }
        None => Redirect::to(SIGNIN_PAGE).into_response(),
    })
}
