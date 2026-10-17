//! The extractors every handler takes its input through. They are axum's `Json`, `Path` and `Query` with one difference: a request that cannot
//! be read is answered in the shape every other error has (`{"error": {"code", "message", "details"}}`, `application/json`), not with
//! axum's bare text. A handler imports them where it used to import axum's (`use crate::json::{Json, Path, Query}`) and is otherwise unchanged;
//! `Json` is also what a handler answers with.

use crate::error::ApiError;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, OptionalFromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// A body that may be absent (`Option<Json<T>>`): no `Content-Type` is `None`, a body that is there is judged like any other.
impl<T, S> OptionalFromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        let got = <axum::Json<T> as OptionalFromRequest<S>>::from_request(req, state).await;
        got.map(|o| o.map(|j| Json(j.0))).map_err(ApiError::from)
    }
}

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct Query<T>(pub T);

/// The field a deserialisation error is about: the path serde walked (`ops[0].op: unknown variant …`), or the name a message gives
/// (`missing field `name``, `unknown field `x``).
fn field_of(message: &str) -> Option<String> {
    let after = message.split_once(": ").map(|(_, rest)| rest).unwrap_or(message);
    let looks_like_path = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '[' | ']' | '-'));
    if let Some((path, _)) = after.split_once(": ") {
        if looks_like_path(path) {
            return Some(path.to_string());
        }
    }
    for marker in ["missing field `", "unknown field `"] {
        if let Some(rest) = after.split(marker).nth(1) {
            if let Some((name, _)) = rest.split_once('`') {
                return Some(name.to_string());
            }
        }
    }
    None
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        match r {
            JsonRejection::JsonDataError(e) => {
                let text = e.body_text();
                let e = ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_body", text.clone());
                match field_of(&text) {
                    Some(field) => e.with_details(serde_json::json!({ "field": field })),
                    None => e,
                }
            }
            JsonRejection::JsonSyntaxError(e) => ApiError::bad_request("invalid_json", e.body_text()),
            JsonRejection::MissingJsonContentType(_) => ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "send the body as Content-Type: application/json"),
            JsonRejection::BytesRejection(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "the request body is larger than the limit (limits.maxBodyMb)")
            }
            other => ApiError::bad_request("invalid_body", other.body_text()),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        ApiError::bad_request("invalid_path", r.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        ApiError::bad_request("invalid_query", r.body_text())
    }
}
