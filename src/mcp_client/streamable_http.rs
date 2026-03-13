//! Streamable HTTP client adapter for rmcp.
//!
//! Implements `StreamableHttpClient` using the existing reqwest 0.12 dependency,
//! avoiding a second reqwest version from rmcp's `transport-streamable-http-client-reqwest` feature.

use super::resolve_env_vars;

use anyhow::Result;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use http::{HeaderName, HeaderValue};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::common::http_header::{
    EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_MCP_PROTOCOL_VERSION, HEADER_SESSION_ID,
    JSON_MIME_TYPE,
};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig, StreamableHttpError,
    StreamableHttpPostResponse,
};
use sse_stream::{Error as SseError, Sse, SseStream};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ReqwestClient — wraps reqwest::Client, implements StreamableHttpClient
// ---------------------------------------------------------------------------

/// Reserved headers that must not be overridden by user-supplied custom headers.
const RESERVED_HEADERS: &[&str] = &[
    "accept",
    HEADER_SESSION_ID,
    HEADER_MCP_PROTOCOL_VERSION,
    HEADER_LAST_EVENT_ID,
];

#[derive(Clone, Debug, Default)]
pub struct ReqwestClient(pub reqwest::Client);

impl rmcp::transport::streamable_http_client::StreamableHttpClient for ReqwestClient {
    type Error = reqwest::Error;

    fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> impl std::future::Future<
        Output = std::result::Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>>,
    > + Send
    + '_ {
        async move {
            let mut request = self
                .0
                .post(uri.as_ref())
                .header("Accept", format!("{EVENT_STREAM_MIME_TYPE}, {JSON_MIME_TYPE}"));

            if let Some(auth_header) = auth_token {
                request = request.bearer_auth(auth_header);
            }
            request = apply_custom_headers(request, custom_headers)?;
            if let Some(session_id) = session_id {
                request = request.header(HEADER_SESSION_ID, session_id.as_ref());
            }

            let response = request
                .json(&message)
                .send()
                .await
                .map_err(StreamableHttpError::Client)?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                if let Some(header) = response.headers().get("www-authenticate") {
                    let header = header
                        .to_str()
                        .map_err(|_| {
                            StreamableHttpError::<reqwest::Error>::UnexpectedServerResponse(
                                Cow::from("invalid www-authenticate header value"),
                            )
                        })?
                        .to_string();
                    return Err(StreamableHttpError::AuthRequired(
                        rmcp::transport::streamable_http_client::AuthRequiredError {
                            www_authenticate_header: header,
                        },
                    ));
                }
            }

            let status = response.status();
            if matches!(
                status,
                reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
            ) {
                return Ok(StreamableHttpPostResponse::Accepted);
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .cloned();
            let session_id = response
                .headers()
                .get(HEADER_SESSION_ID)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            match content_type {
                Some(ref ct)
                    if ct
                        .as_bytes()
                        .starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) =>
                {
                    let event_stream =
                        SseStream::from_byte_stream(response.bytes_stream()).boxed();
                    Ok(StreamableHttpPostResponse::Sse(event_stream, session_id))
                }
                Some(ref ct) if ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {
                    match response.json::<ServerJsonRpcMessage>().await {
                        Ok(message) => {
                            Ok(StreamableHttpPostResponse::Json(message, session_id))
                        }
                        Err(_e) => Ok(StreamableHttpPostResponse::Accepted),
                    }
                }
                _ => Err(StreamableHttpError::UnexpectedContentType(
                    content_type
                        .map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string()),
                )),
            }
        }
    }

    fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> impl std::future::Future<
        Output = std::result::Result<(), StreamableHttpError<Self::Error>>,
    > + Send
    + '_ {
        async move {
            let mut request = self.0.delete(uri.as_ref());
            if let Some(auth_header) = auth_token {
                request = request.bearer_auth(auth_header);
            }
            request = request.header(HEADER_SESSION_ID, session_id.as_ref());
            request = apply_custom_headers(request, custom_headers)?;
            let response = request.send().await.map_err(StreamableHttpError::Client)?;

            if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                return Err(StreamableHttpError::ServerDoesNotSupportDeleteSession);
            }
            let _response = response
                .error_for_status()
                .map_err(StreamableHttpError::Client)?;
            Ok(())
        }
    }

    fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> impl std::future::Future<
        Output = std::result::Result<
            BoxStream<'static, std::result::Result<Sse, SseError>>,
            StreamableHttpError<Self::Error>,
        >,
    > + Send
    + '_ {
        async move {
            let mut request = self
                .0
                .get(uri.as_ref())
                .header(
                    "Accept",
                    format!("{EVENT_STREAM_MIME_TYPE}, {JSON_MIME_TYPE}"),
                )
                .header(HEADER_SESSION_ID, session_id.as_ref());

            if let Some(last_event_id) = last_event_id {
                request = request.header(HEADER_LAST_EVENT_ID, last_event_id);
            }
            if let Some(auth_header) = auth_token {
                request = request.bearer_auth(auth_header);
            }
            request = apply_custom_headers(request, custom_headers)?;

            let response = request.send().await.map_err(StreamableHttpError::Client)?;
            if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                return Err(StreamableHttpError::ServerDoesNotSupportSse);
            }
            let response = response
                .error_for_status()
                .map_err(StreamableHttpError::Client)?;
            let event_stream = SseStream::from_byte_stream(response.bytes_stream()).boxed();
            Ok(event_stream)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn apply_custom_headers(
    mut builder: reqwest::RequestBuilder,
    custom_headers: HashMap<HeaderName, HeaderValue>,
) -> std::result::Result<reqwest::RequestBuilder, StreamableHttpError<reqwest::Error>> {
    for (name, value) in custom_headers {
        if RESERVED_HEADERS
            .iter()
            .any(|&r| name.as_str().eq_ignore_ascii_case(r))
        {
            if name
                .as_str()
                .eq_ignore_ascii_case(HEADER_MCP_PROTOCOL_VERSION)
            {
                builder = builder.header(name, value);
                continue;
            }
            return Err(StreamableHttpError::ReservedHeaderConflict(
                name.to_string(),
            ));
        }
        builder = builder.header(name, value);
    }
    Ok(builder)
}

// ---------------------------------------------------------------------------
// Public API: build a transport from endpoint + headers
// ---------------------------------------------------------------------------

pub fn build_transport(
    endpoint: &str,
    headers: &HashMap<String, String>,
) -> Result<StreamableHttpClientTransport<ReqwestClient>> {
    let resolved_headers = resolve_env_vars(headers);

    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint);

    let mut custom_headers = HashMap::new();
    for (k, v) in &resolved_headers {
        if k.eq_ignore_ascii_case("authorization") {
            let token = v
                .strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .unwrap_or(v);
            config.auth_header = Some(token.to_string());
        } else if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            custom_headers.insert(name, value);
        }
    }
    if !custom_headers.is_empty() {
        config.custom_headers = custom_headers;
    }

    Ok(StreamableHttpClientTransport::with_client(
        ReqwestClient(reqwest::Client::new()),
        config,
    ))
}
