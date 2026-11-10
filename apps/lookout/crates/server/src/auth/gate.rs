use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::auth::signed_in::SignedIn;

const SIGNIN_PAGE: &str = "/signin/";

pub async fn require_signin(signed_in: Option<SignedIn>, request: Request, next: Next) -> Response {
    match signed_in {
        Some(_) => next.run(request).await,
        None => Redirect::to(SIGNIN_PAGE).into_response(),
    }
}
