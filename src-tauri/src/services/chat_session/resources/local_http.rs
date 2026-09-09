//! Loopback-only streaming delivery to the WebView. No arbitrary URL, cookies or filesystem paths.
use super::*;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{Bytes, Frame, Incoming},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, net::SocketAddr};

type Body = UnsyncBoxBody<Bytes, ChatResourceError>;

#[derive(Debug)]
pub(super) struct LocalEndpoint {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LocalEndpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ChatSessionService {
    pub(super) async fn resource_endpoint(self: &Arc<Self>) -> Result<String, ChatResourceError> {
        let mut endpoint = self.resources.endpoint.lock().await;
        if endpoint.is_none() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|_| ChatResourceError::Network)?;
            let address = listener
                .local_addr()
                .map_err(|_| ChatResourceError::Network)?;
            let service = Arc::downgrade(self);
            let task = tokio::spawn(async move {
                let connections = Arc::new(Semaphore::new(80));
                let mut tasks = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                        accepted = listener.accept() => {
                            let Ok((socket, _)) = accepted else { break };
                            let Ok(permit) = connections.clone().try_acquire_owned() else { continue };
                            let service = service.clone();
                            tasks.spawn(async move {
                                let _permit = permit;
                                let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel::<ResourceCancellation>();
                                let cancel_sender = std::sync::Mutex::new(Some(cancel_sender));
                                let handler = service_fn(move |request| {
                                    let service = service.clone();
                                    let cancel_sender = cancel_sender.lock().unwrap().take();
                                    async move {
                                        let response = match service.upgrade() {
                                            Some(service) => service.local_resource(request, address, cancel_sender).await,
                                            None => error_response(ChatResourceError::Cancelled),
                                        };
                                        Ok::<_, Infallible>(response)
                                    }
                                });
                                let mut builder = http1::Builder::new();
                                builder
                                    .keep_alive(false)
                                    .max_buf_size(16 * 1024)
                                    .timer(TokioTimer::new())
                                    .header_read_timeout(Duration::from_secs(5));
                                let connection = builder.serve_connection(TokioIo::new(socket), handler);
                                // Revocation must interrupt the socket even when a paused media
                                // consumer applies backpressure and Hyper stops polling the body.
                                let revoked = async move {
                                    if let Ok(cancellation) = cancel_receiver.await {
                                        let _ = cancellation.run(std::future::pending::<Result<(), ChatResourceError>>()).await;
                                    } else {
                                        std::future::pending::<()>().await;
                                    }
                                };
                                tokio::select! {
                                    biased;
                                    _ = revoked => {},
                                    _ = tokio::time::timeout(Duration::from_secs(1800), connection) => {},
                                }
                            });
                        }
                    }
                }
            });
            *endpoint = Some(LocalEndpoint { address, task });
        }
        Ok(format!("http://{}", endpoint.as_ref().unwrap().address))
    }

    async fn local_resource(
        &self,
        request: Request<Incoming>,
        address: SocketAddr,
        cancel_sender: Option<tokio::sync::oneshot::Sender<ResourceCancellation>>,
    ) -> Response<Body> {
        let origin = request
            .headers()
            .get("origin")
            .and_then(|value| value.to_str().ok());
        // The unguessable path is a revocable capability. Host checks also reject DNS rebinding.
        if request
            .headers()
            .get("host")
            .and_then(|value| value.to_str().ok())
            != Some(address.to_string().as_str())
            || request.uri().query().is_some()
            || origin.is_some_and(|origin| !allowed_origin(origin))
            || !matches!(request.method().as_str(), "GET" | "HEAD")
        {
            return error_response(ChatResourceError::Invalid);
        }
        let result = self.serve_resource(&request, cancel_sender).await;
        let mut response = result.unwrap_or_else(|error| {
            log::warn!("chat resource stage=read error={error}");
            if !matches!(error, ChatResourceError::Cancelled) {
                if let Some(entry) = request
                    .uri()
                    .path()
                    .strip_prefix("/resource/")
                    .and_then(|id| self.resources.entries.lock().unwrap().get(id).cloned())
                {
                    let _ = self.resources.diagnostics.send(ChatResourceDiagnostic {
                        lease_id: entry.lease_id.clone(),
                        error,
                    });
                }
            }
            error_response(error)
        });
        let headers = response.headers_mut();
        headers.insert("cache-control", "no-store, private".parse().unwrap());
        headers.insert("pragma", "no-cache".parse().unwrap());
        headers.insert("x-content-type-options", "nosniff".parse().unwrap());
        headers.insert(
            "content-security-policy",
            "default-src 'none'; sandbox".parse().unwrap(),
        );
        headers.insert("referrer-policy", "no-referrer".parse().unwrap());
        if let Some(origin) = origin {
            headers.insert("access-control-allow-origin", origin.parse().unwrap());
            headers.insert("vary", "Origin".parse().unwrap());
        }
        response
    }

    async fn serve_resource(
        &self,
        request: &Request<Incoming>,
        cancel_sender: Option<tokio::sync::oneshot::Sender<ResourceCancellation>>,
    ) -> Result<Response<Body>, ChatResourceError> {
        let id = request
            .uri()
            .path()
            .strip_prefix("/resource/")
            .ok_or(ChatResourceError::Invalid)?;
        let entry = self
            .resources
            .entries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ChatResourceError::NotFound)?;
        let (_, cancellation) = self.resource_context(&entry).await?;
        if let Some(sender) = cancel_sender {
            let _ = sender.send(cancellation);
        }
        let range = request
            .headers()
            .get("range")
            .map(|value| value.to_str().map_err(|_| ChatResourceError::Invalid))
            .transpose()?;
        let stream = self.resource_stream(&entry, range).await?;
        let status = stream.response.status();
        let mut response = Response::builder().status(status);
        // No upstream cache, redirect, auth or Set-Cookie headers cross this boundary.
        for name in ["content-length", "content-range", "accept-ranges"] {
            if let Some(value) = stream.response.headers().get(name) {
                response = response.header(name, value);
            }
        }
        response = response.header("content-type", safe_content_type(&stream.response));
        if range.is_some() && status == reqwest::StatusCode::OK {
            log::debug!("chat resource range=unsupported_by_proxy");
            response = response.header("accept-ranges", "none");
        }
        let body = if request.method() == hyper::Method::HEAD {
            empty_body()
        } else {
            StreamBody::new(futures_util::stream::try_unfold(
                stream,
                |mut stream| async {
                    stream
                        .chunk()
                        .await
                        .map(|chunk| chunk.map(|bytes| (Frame::data(bytes), stream)))
                },
            ))
            .boxed_unsync()
        };
        response.body(body).map_err(|_| ChatResourceError::Invalid)
    }
}

fn safe_content_type(response: &reqwest::Response) -> &str {
    let value = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap()
        .trim();
    match value {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/avif" | "audio/mpeg"
        | "audio/mp4" | "audio/ogg" | "audio/wav" | "audio/x-wav" | "audio/webm" | "video/mp4"
        | "video/webm" | "video/ogg" | "video/quicktime" | "application/pdf" => value,
        _ => "application/octet-stream",
    }
}

fn allowed_origin(origin: &str) -> bool {
    matches!(
        origin,
        "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
    ) || (cfg!(debug_assertions)
        && matches!(origin, "http://localhost:1420" | "http://127.0.0.1:1420"))
}

fn empty_body() -> Body {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed_unsync()
}

fn error_response(error: ChatResourceError) -> Response<Body> {
    let status = match error {
        ChatResourceError::Permission => 403,
        ChatResourceError::NotFound | ChatResourceError::Cancelled => 404,
        ChatResourceError::Invalid => 400,
        ChatResourceError::TooLarge => 413,
        ChatResourceError::Busy => 429,
        ChatResourceError::Timeout => 504,
        _ => 502,
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(
            Full::new(Bytes::from(serde_json::to_vec(&error).unwrap()))
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .unwrap()
}
