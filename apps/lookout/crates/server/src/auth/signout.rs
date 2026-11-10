use axum::http::StatusCode;
use tower_sessions::Session;

use crate::auth::AuthError;

pub async fn signout(session: Session) -> Result<StatusCode, AuthError> {
    session.flush().await?;
    Ok(StatusCode::NO_CONTENT)
}
