//! Identity-bound resource capabilities. Front owns storage signing and access to private networks.
mod files;
mod local_http;

use super::*;
use std::future::Future;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_HANDLES: usize = 512;
const RESOURCE_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, Serialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ChatResourceError {
    #[error("Resource request cancelled")]
    Cancelled,
    #[error("Resource access denied")]
    Permission,
    #[error("Resource not found")]
    NotFound,
    #[error("Resource service is unavailable")]
    Network,
    #[error("Resource request timed out")]
    Timeout,
    #[error("Invalid resource request")]
    Invalid,
    #[error("Resource exceeds the 1 GiB limit")]
    TooLarge,
    #[error("Resource queue is full")]
    Busy,
    #[error("Unable to save the resource")]
    Save,
    #[error("This file type cannot be opened automatically; use Download")]
    Unsupported,
}

impl From<ManagerError> for ChatResourceError {
    fn from(error: ManagerError) -> Self {
        match error {
            ManagerError::ContextChanged => Self::Cancelled,
            ManagerError::NotFoundOrNoPermission
            | ManagerError::Forbidden
            | ManagerError::Unauthorized
            | ManagerError::NoSession
            | ManagerError::TokenExchange { .. } => Self::Permission,
            ManagerError::NotFound => Self::NotFound,
            // Do not forward upstream messages, which can contain credentials or URLs.
            _ => Self::Network,
        }
    }
}

fn network_error(error: reqwest::Error) -> ChatResourceError {
    if error.is_timeout() {
        ChatResourceError::Timeout
    } else {
        ChatResourceError::Network
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResourceHandle {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResourceDiagnostic {
    pub lease_id: String,
    pub error: ChatResourceError,
}

#[derive(Debug)]
struct ResourceEntry {
    lease_id: String,
    uri: String,
    cancelled: watch::Sender<bool>,
}

#[derive(Debug)]
pub(super) struct ResourceState {
    entries: std::sync::Mutex<HashMap<String, Arc<ResourceEntry>>>,
    endpoint: Mutex<Option<local_http::LocalEndpoint>>,
    slots: Arc<Semaphore>,
    demand: Arc<Semaphore>,
    http: reqwest::Client,
    opened: std::sync::Mutex<HashMap<String, Vec<tempfile::TempPath>>>,
    diagnostics: tokio::sync::broadcast::Sender<ChatResourceDiagnostic>,
}

impl ResourceState {
    pub(super) fn new() -> Self {
        Self {
            entries: Default::default(),
            endpoint: Mutex::new(None),
            slots: Arc::new(Semaphore::new(RESOURCE_CONCURRENCY)),
            demand: Arc::new(Semaphore::new(68)),
            http: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .tcp_keepalive(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("resource HTTP client"),
            opened: Default::default(),
            diagnostics: tokio::sync::broadcast::channel(32).0,
        }
    }

    pub(super) fn revoke_lease(&self, lease_id: &str) {
        self.entries.lock().unwrap().retain(|_, entry| {
            if entry.lease_id != lease_id {
                return true;
            }
            entry.cancelled.send_replace(true);
            false
        });
        self.opened.lock().unwrap().remove(lease_id);
    }
}

#[derive(Clone)]
struct ResourceCancellation {
    resource: watch::Receiver<bool>,
    lease: watch::Receiver<bool>,
}

impl ResourceCancellation {
    fn check(&self) -> Result<(), ChatResourceError> {
        if *self.resource.borrow() || *self.lease.borrow() {
            Err(ChatResourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn run<T>(
        &self,
        work: impl Future<Output = Result<T, ChatResourceError>>,
    ) -> Result<T, ChatResourceError> {
        self.check()?;
        let mut resource = self.resource.clone();
        let mut lease = self.lease.clone();
        tokio::select! {
            biased;
            _ = resource.changed() => Err(ChatResourceError::Cancelled),
            _ = lease.changed() => Err(ChatResourceError::Cancelled),
            result = work => { self.check()?; result }
        }
    }
}

struct ResourceStream {
    response: reqwest::Response,
    cancellation: ResourceCancellation,
    _slot: OwnedSemaphorePermit,
    _demand: OwnedSemaphorePermit,
    received: u64,
    diagnostics: tokio::sync::broadcast::Sender<ChatResourceDiagnostic>,
    lease_id: String,
}

impl ResourceStream {
    async fn chunk(&mut self) -> Result<Option<hyper::body::Bytes>, ChatResourceError> {
        let result = self
            .cancellation
            .run(async { self.response.chunk().await.map_err(network_error) })
            .await
            .and_then(|chunk| {
                if let Some(bytes) = &chunk {
                    self.received += bytes.len() as u64;
                    if self.received > MAX_RESOURCE_BYTES {
                        return Err(ChatResourceError::TooLarge);
                    }
                }
                Ok(chunk)
            });
        let chunk = result.inspect_err(|error| {
            if !matches!(error, ChatResourceError::Cancelled) {
                log::warn!("chat resource stage=stream error={error}");
                let _ = self.diagnostics.send(ChatResourceDiagnostic {
                    lease_id: self.lease_id.clone(),
                    error: *error,
                });
            }
        })?;
        Ok(chunk)
    }
}

impl ChatSessionService {
    pub fn subscribe_resource_errors(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ChatResourceDiagnostic> {
        self.resources.diagnostics.subscribe()
    }

    pub async fn register_resource(
        self: &Arc<Self>,
        lease_id: &str,
        uri: &str,
    ) -> Result<ChatResourceHandle, ChatResourceError> {
        super::transfers::validate_resource_uri(uri).map_err(|_| ChatResourceError::Invalid)?;
        let lease = self.lease(lease_id).await?;
        let endpoint = self.resource_endpoint().await?;
        let current = lease.lock().await;
        if *current.cancelled.borrow() {
            return Err(ChatResourceError::Cancelled);
        }
        let mut entries = self.resources.entries.lock().unwrap();
        if entries.len() >= MAX_HANDLES {
            return Err(ChatResourceError::Busy);
        }
        let id = Uuid::new_v4().to_string();
        entries.insert(
            id.clone(),
            Arc::new(ResourceEntry {
                lease_id: lease_id.into(),
                uri: uri.into(),
                cancelled: watch::channel(false).0,
            }),
        );
        Ok(ChatResourceHandle {
            url: format!("{endpoint}/resource/{id}"),
            id,
        })
    }

    pub fn release_resource(&self, lease_id: &str, id: &str) {
        let mut entries = self.resources.entries.lock().unwrap();
        if entries
            .get(id)
            .is_some_and(|entry| entry.lease_id == lease_id)
        {
            if let Some(entry) = entries.remove(id) {
                entry.cancelled.send_replace(true);
            }
        }
    }

    fn resource_entry(
        &self,
        lease_id: &str,
        id: &str,
    ) -> Result<Arc<ResourceEntry>, ChatResourceError> {
        self.resources
            .entries
            .lock()
            .unwrap()
            .get(id)
            .filter(|entry| entry.lease_id == lease_id)
            .cloned()
            .ok_or(ChatResourceError::Cancelled)
    }

    async fn resource_context(
        &self,
        entry: &ResourceEntry,
    ) -> Result<(ResolvedChatTarget, ResourceCancellation), ChatResourceError> {
        let lease = self.lease(&entry.lease_id).await?;
        let current = lease.lock().await;
        let cancellation = ResourceCancellation {
            resource: entry.cancelled.subscribe(),
            lease: current.cancelled.subscribe(),
        };
        cancellation.check()?;
        Ok((current.target.clone(), cancellation))
    }

    async fn resource_stream(
        &self,
        entry: &ResourceEntry,
        range: Option<&str>,
    ) -> Result<ResourceStream, ChatResourceError> {
        if let Some(range) = range {
            validate_range(range)?;
        }
        let (target, cancellation) = self.resource_context(entry).await?;
        let demand = self
            .resources
            .demand
            .clone()
            .try_acquire_owned()
            .map_err(|_| ChatResourceError::Busy)?;
        cancellation
            .run(async {
                // FIFO semaphore waits are event driven and cancellable. Uploads have separate slots.
                let slot = self
                    .resources
                    .slots
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| ChatResourceError::Cancelled)?;
                let manager = self.manager()?;
                let token = manager
                    .exchange_token_for_generation(
                        target.manager_generation,
                        &target.robot_account_id,
                        Some("config:read".into()),
                    )
                    .await?;
                cancellation.check()?;
                let mut url = target
                    .http_base_url
                    .join("resource/s3")
                    .map_err(|_| ChatResourceError::Invalid)?;
                url.query_pairs_mut().append_pair("s3uri", &entry.uri);
                let mut request = self
                    .resources
                    .http
                    .get(url)
                    .header(
                        COOKIE,
                        frontend_session_cookie(&target, &token.access_token)
                            .map_err(|_| ChatResourceError::Invalid)?,
                    )
                    .header(ACCEPT, "*/*")
                    .header("Accept-Encoding", "identity")
                    .header("Cache-Control", "no-cache, no-store");
                for (name, value) in &target.routing_headers {
                    request = request.header(name, value);
                }
                if let Some(range) = range {
                    request = request.header("Range", range);
                }
                let response = request.send().await.map_err(network_error)?;
                let status = response.status().as_u16();
                log::debug!("chat resource stage=proxy status={status}");
                match status {
                    200 | 206 | 416 => {}
                    401 | 403 => return Err(ChatResourceError::Permission),
                    404 => return Err(ChatResourceError::NotFound),
                    _ => return Err(ChatResourceError::Network),
                }
                if response
                    .content_length()
                    .is_some_and(|size| size > MAX_RESOURCE_BYTES)
                {
                    return Err(ChatResourceError::TooLarge);
                }
                Ok(ResourceStream {
                    response,
                    cancellation: cancellation.clone(),
                    _slot: slot,
                    _demand: demand,
                    received: 0,
                    diagnostics: self.resources.diagnostics.clone(),
                    lease_id: entry.lease_id.clone(),
                })
            })
            .await
    }
}

fn validate_range(value: &str) -> Result<(), ChatResourceError> {
    let valid = value
        .strip_prefix("bytes=")
        .and_then(|value| value.split_once('-'))
        .is_some_and(|(start, end)| {
            (!start.is_empty() || !end.is_empty())
                && (start.is_empty() || start.parse::<u64>().is_ok())
                && (end.is_empty() || end.parse::<u64>().is_ok())
                && (start.is_empty()
                    || end.is_empty()
                    || start.parse::<u64>().unwrap() <= end.parse::<u64>().unwrap())
        });
    if value.len() > 64 || !valid {
        Err(ChatResourceError::Invalid)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "resource_tests.rs"]
mod tests;
