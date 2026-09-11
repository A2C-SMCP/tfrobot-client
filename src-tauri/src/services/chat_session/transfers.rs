//! Bounded, cancellable platform transport. Kit still owns the upload protocol and result.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;

const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
const MAX_PENDING_PER_LEASE: usize = 4;
const TRANSFER_LIFETIME: Duration = Duration::from_secs(30);
const UPLOAD_PATH: &str = "v1/dashboard/remote/source/cos/upload";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatUploadFile {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug)]
pub(super) struct PendingTransfer {
    lease_id: String,
    created_at: Instant,
    started: bool,
    cancelled: watch::Sender<bool>,
}

struct TransferGuard<'a> {
    service: &'a ChatSessionService,
    id: &'a str,
}

impl Drop for TransferGuard<'_> {
    fn drop(&mut self) {
        self.service.transfers.lock().unwrap().remove(self.id);
    }
}

fn invalid(message: &str) -> ManagerError {
    ManagerError::InvalidResponse(message.into())
}

impl ChatUploadFile {
    fn into_part(self) -> Result<reqwest::multipart::Part, ManagerError> {
        if self.name.is_empty()
            || self.name.len() > 255
            || self
                .name
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
            || self.mime_type.len() > 255
            || self.data_base64.len() > MAX_FILE_BYTES.div_ceil(3) * 4
        {
            return Err(invalid(
                "Invalid attachment metadata or attachment exceeds 10 MiB",
            ));
        }
        let bytes = STANDARD
            .decode(self.data_base64)
            .map_err(|_| invalid("Invalid attachment encoding"))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(invalid("Attachment exceeds 10 MiB"));
        }
        reqwest::multipart::Part::bytes(bytes)
            .file_name(self.name)
            .mime_str(&self.mime_type)
            .map_err(|_| invalid("Invalid attachment media type"))
    }
}

impl ChatSessionService {
    /// Reserve before sending bytes so cancellation cannot overtake registration.
    pub async fn prepare_transfer(&self, lease_id: &str) -> Result<String, ManagerError> {
        let lease = self.lease(lease_id).await?;
        let current = lease.lock().await;
        if *current.cancelled.borrow() {
            return Err(ManagerError::ContextChanged);
        }
        let mut transfers = self.transfers.lock().unwrap();
        transfers
            .retain(|_, entry| entry.started || entry.created_at.elapsed() < TRANSFER_LIFETIME);
        if transfers
            .values()
            .filter(|entry| entry.lease_id == lease_id)
            .count()
            >= MAX_PENDING_PER_LEASE
        {
            return Err(invalid(
                "Too many concurrent chat transfers; retry when one finishes",
            ));
        }
        let id = Uuid::new_v4().to_string();
        transfers.insert(
            id.clone(),
            PendingTransfer {
                lease_id: lease_id.into(),
                created_at: Instant::now(),
                started: false,
                cancelled: watch::channel(false).0,
            },
        );
        Ok(id)
    }

    pub fn cancel_transfer(&self, lease_id: &str, id: &str) {
        let mut transfers = self.transfers.lock().unwrap();
        if transfers
            .get(id)
            .is_some_and(|entry| entry.lease_id == lease_id)
        {
            if let Some(entry) = transfers.remove(id) {
                entry.cancelled.send_replace(true);
            }
        }
    }

    pub(super) fn cancel_transfers(&self, lease_id: &str) {
        self.transfers.lock().unwrap().retain(|_, entry| {
            if entry.lease_id != lease_id {
                return true;
            }
            entry.cancelled.send_replace(true);
            false
        });
    }

    async fn transfer(
        &self,
        lease_id: &str,
        id: &str,
        required_scope: &str,
        build_request: impl FnOnce(
            &ResolvedChatTarget,
            &ChatSessionCredential,
        ) -> Result<reqwest::RequestBuilder, ManagerError>,
    ) -> Result<ChatHttpResponse, ManagerError> {
        let (mut cancelled, deadline) = {
            let mut transfers = self.transfers.lock().unwrap();
            let entry = transfers
                .get_mut(id)
                .filter(|entry| entry.lease_id == lease_id)
                .ok_or(ManagerError::ContextChanged)?;
            if entry.started || entry.created_at.elapsed() >= TRANSFER_LIFETIME {
                return Err(invalid("Chat transfer is already started or expired"));
            }
            entry.started = true;
            (
                entry.cancelled.subscribe(),
                entry.created_at + TRANSFER_LIFETIME,
            )
        };
        let _guard = TransferGuard { service: self, id };
        let work = async {
            let lease = self.lease(lease_id).await?;
            let (target, mut lease_cancelled) = {
                let current = lease.lock().await;
                (current.target.clone(), current.cancelled.subscribe())
            };
            // Upload/read use the existing Server permission contract. These credentials are
            // operation-local and never replace the lease's chat/Socket credential.
            let exchanged = self
                .manager()?
                .exchange_token_for_generation(
                    target.manager_generation,
                    &target.robot_account_id,
                    Some(required_scope.into()),
                )
                .await?;
            let credential = CachedToken::new(exchanged).credential();
            let request = build_request(&target, &credential)?;
            self.read_response(request, &mut lease_cancelled).await
        };
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(ManagerError::ContextChanged),
            result = tokio::time::timeout_at(deadline.into(), work) => {
                result.map_err(|_| invalid("Chat transfer timed out"))?
            }
        }
    }

    pub async fn upload(
        &self,
        lease_id: &str,
        id: &str,
        request_url: &str,
        file: ChatUploadFile,
    ) -> Result<ChatHttpResponse, ManagerError> {
        self.transfer(lease_id, id, "config:write", |target, credential| {
            let url = target
                .http_base_url
                .join(UPLOAD_PATH)
                .map_err(|_| invalid("Invalid upload endpoint"))?;
            if url.as_str() != request_url {
                return Err(invalid("Chat upload rejected an unsupported route"));
            }
            let form = reqwest::multipart::Form::new().part("file", file.into_part()?);
            Ok(self
                .authorized_request(target, url, reqwest::Method::POST, credential)?
                .multipart(form))
        })
        .await
    }
}

pub(super) fn validate_resource_uri(uri: &str) -> Result<(), ManagerError> {
    let url = Url::parse(uri).map_err(|_| invalid("Invalid chat resource URI"))?;
    if uri.len() > 4096
        || uri.chars().any(char::is_control)
        || url.scheme() != "s3"
        || url.host_str().is_none()
        || url.path().len() < 2
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("Invalid chat resource URI"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
pub(super) mod tests;
