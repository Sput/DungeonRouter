use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::AppState;

#[derive(Clone)]
pub struct SupabaseAuth {
    client: Client,
    user_endpoint: Url,
    publishable_key: String,
}

impl SupabaseAuth {
    pub fn new(
        supabase_url: &str,
        publishable_key: impl Into<String>,
    ) -> Result<Self, AuthConfigurationError> {
        let base_url = Url::parse(supabase_url.trim())?;
        let user_endpoint = base_url.join("auth/v1/user")?;
        let publishable_key = publishable_key.into();
        if publishable_key.trim().is_empty() {
            return Err(AuthConfigurationError::MissingPublishableKey);
        }
        Ok(Self {
            client: Client::new(),
            user_endpoint,
            publishable_key,
        })
    }

    async fn verify(&self, token: &str) -> Result<AuthenticatedUser, AuthError> {
        let response = self
            .client
            .get(self.user_endpoint.clone())
            .header("apikey", &self.publishable_key)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| AuthError::Unavailable)?;
        if !response.status().is_success() {
            return Err(AuthError::Unauthorized);
        }
        response
            .json::<AuthenticatedUser>()
            .await
            .map_err(|_| AuthError::Unauthorized)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
}

pub async fn require_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(auth) = &state.auth else {
        return next.run(request).await;
    };
    let token = match bearer_token(&headers) {
        Some(token) => token,
        None => return AuthError::Unauthorized.into_response(),
    };
    match auth.verify(token).await {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

#[derive(Debug, Error)]
pub enum AuthConfigurationError {
    #[error("SUPABASE_URL must be a valid URL")]
    InvalidUrl(#[from] url::ParseError),
    #[error("SUPABASE_PUBLISHABLE_KEY must not be empty")]
    MissingPublishableKey,
}

#[derive(Debug, Error)]
enum AuthError {
    #[error("authentication required")]
    Unauthorized,
    #[error("authentication service unavailable")]
    Unavailable,
}

#[derive(Serialize)]
struct AuthErrorBody {
    error: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        (
            status,
            Json(AuthErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extracts_only_non_empty_bearer_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer signed-token"),
        );
        assert_eq!(bearer_token(&headers), Some("signed-token"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Basic credentials"),
        );
        assert_eq!(bearer_token(&headers), None);
    }
}
