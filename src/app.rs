use axum::{
    Router,
    body::{Body, Bytes},
    extract::{FromRequestParts, Request, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message as TungsteniteMessage, frame::coding::CloseCode},
    },
};
use tower_http::cors::{Any, CorsLayer};
use tracing::error;

use crate::config::Config;
use crate::ffi;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub backend_url: String,
    client: Client,
    _server: std::sync::Arc<EmbeddedServer>,
}

pub fn build_router(state: AppState) -> Router {
    build_router_without_cors(state).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
}

pub fn build_router_without_cors(state: AppState) -> Router {
    let docs = Router::new()
        .route("/docs", any(proxy_handler))
        .route("/docs/{*path}", any(proxy_handler))
        .route("/openapi.json", any(proxy_handler));

    Router::new()
        .route("/health", any(proxy_handler))
        .route("/extension/icon/{apk_name}", any(proxy_handler))
        .route("/api/v1", any(proxy_handler))
        .route("/api/v1/{*path}", any(proxy_handler))
        .merge(docs)
        .with_state(state)
}

pub(crate) fn new_state(config: Config, backend_url: String, handle: *mut ffi::ManatanServerHandle) -> AppState {
    AppState {
        config,
        backend_url,
        client: Client::new(),
        _server: std::sync::Arc::new(EmbeddedServer { handle }),
    }
}

struct EmbeddedServer {
    handle: *mut ffi::ManatanServerHandle,
}

unsafe impl Send for EmbeddedServer {}
unsafe impl Sync for EmbeddedServer {}

impl Drop for EmbeddedServer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { ffi::manatan_server_stop(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

async fn proxy_handler(State(state): State<AppState>, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();
    let is_ws = parts
        .headers
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    if is_ws {
        let path_query = parts
            .uri
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or(parts.uri.path());
        let backend_ws = backend_ws_url(&state.backend_url);
        let backend_url = format!("{backend_ws}{path_query}");
        let headers = parts.headers.clone();
        let protocols: Vec<String> = parts
            .headers
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
            Ok(ws) => {
                return ws
                    .protocols(protocols)
                    .on_upgrade(move |socket| handle_socket(socket, headers, backend_url))
                    .into_response();
            }
            Err(err) => return err.into_response(),
        }
    }

    let req = Request::from_parts(parts, body);
    proxy_request(state.client, req, &state.backend_url, "").await
}

async fn handle_socket(client_socket: WebSocket, headers: HeaderMap, backend_url: String) {
    let mut request = match backend_url.clone().into_client_request() {
        Ok(req) => req,
        Err(e) => {
            error!("invalid backend URL {}: {}", backend_url, e);
            return;
        }
    };
    for &name in &[
        "cookie",
        "authorization",
        "user-agent",
        "sec-websocket-protocol",
        "origin",
    ] {
        if let Some(value) = headers.get(name) {
            request.headers_mut().insert(name, value.clone());
        }
    }
    let (backend_socket, _) = match connect_async(request).await {
        Ok(conn) => conn,
        Err(e) => {
            error!("backend ws connect failed: {}", e);
            return;
        }
    };
    let (mut client_sender, mut client_receiver) = client_socket.split();
    let (mut backend_sender, mut backend_receiver) = backend_socket.split();
    loop {
        tokio::select! {
            msg = client_receiver.next() => match msg {
                Some(Ok(msg)) => if let Some(t_msg) = axum_to_tungstenite(msg) {
                    if backend_sender.send(t_msg).await.is_err() { break; }
                },
                _ => break,
            },
            msg = backend_receiver.next() => match msg {
                Some(Ok(msg)) => if client_sender.send(tungstenite_to_axum(msg)).await.is_err() { break; },
                _ => break,
            }
        }
    }
}

async fn proxy_request(
    client: Client,
    req: Request,
    base_url: &str,
    strip_prefix: &str,
) -> Response {
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(req.uri().path());
    let target_path = if !strip_prefix.is_empty() && path_query.starts_with(strip_prefix) {
        &path_query[strip_prefix.len()..]
    } else {
        path_query
    };

    let target_url = format!("{base_url}{target_path}");
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());

    let mut builder = client.request(method, &target_url).body(body);
    for (key, value) in headers.iter() {
        if key.as_str() != "host" {
            builder = builder.header(key, value);
        }
    }

    match builder.send().await {
        Ok(resp) => {
            let mut response_builder = Response::builder().status(resp.status());
            for (key, value) in resp.headers() {
                response_builder = response_builder.header(key, value);
            }
            response_builder
                .body(Body::from_stream(resp.bytes_stream()))
                .unwrap_or_else(|_| Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::empty())
                    .unwrap())
        }
        Err(_err) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Body::empty())
            .unwrap(),
    }
}

fn axum_to_tungstenite(msg: Message) -> Option<TungsteniteMessage> {
    match msg {
        Message::Text(t) => Some(TungsteniteMessage::Text(t.as_str().into())),
        Message::Binary(b) => Some(TungsteniteMessage::Binary(b.to_vec())),
        Message::Ping(p) => Some(TungsteniteMessage::Ping(p.to_vec())),
        Message::Pong(p) => Some(TungsteniteMessage::Pong(p.to_vec())),
        Message::Close(c) => {
            let frame = c.map(|cf| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: CloseCode::from(cf.code),
                reason: cf.reason.to_string().into(),
            });
            Some(TungsteniteMessage::Close(frame))
        }
    }
}

fn tungstenite_to_axum(msg: TungsteniteMessage) -> Message {
    match msg {
        TungsteniteMessage::Text(t) => Message::Text(t.as_str().into()),
        TungsteniteMessage::Binary(b) => Message::Binary(b.into()),
        TungsteniteMessage::Ping(p) => Message::Ping(p.into()),
        TungsteniteMessage::Pong(p) => Message::Pong(p.into()),
        TungsteniteMessage::Close(c) => {
            let frame = c.map(|cf| axum::extract::ws::CloseFrame {
                code: u16::from(cf.code),
                reason: cf.reason.to_string().into(),
            });
            Message::Close(frame)
        }
        TungsteniteMessage::Frame(_) => Message::Binary(Bytes::new()),
    }
}

fn backend_ws_url(base: &str) -> String {
    if let Some(stripped) = base.strip_prefix("https://") {
        format!("wss://{}", stripped)
    } else if let Some(stripped) = base.strip_prefix("http://") {
        format!("ws://{}", stripped)
    } else {
        format!("ws://{}", base)
    }
}
