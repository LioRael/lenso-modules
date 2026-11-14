//! Private file operations with scoped metadata and bounded native streams.
use lenso_app::{ActionError, ErrorCode, Principal, require_principal};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub type Result<T> = std::result::Result<T, ActionError>;
pub type Reader = Box<dyn AsyncRead + Unpin + Send>;
pub type Writer = Box<dyn AsyncWrite + Unpin + Send>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileRecord {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub size: i64,
    pub sha256: String,
    pub created_ms: i64,
}
#[derive(Clone, Copy, Debug)]
pub enum Retired {
    Pending,
    Deleting,
}
impl Retired {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Deleting => "deleting",
        }
    }
}
pub fn unavailable<E: std::fmt::Debug>(_error: E) -> ActionError {
    ActionError::new(ErrorCode::Internal, "File storage unavailable")
}
pub fn missing() -> ActionError {
    ActionError::new(ErrorCode::NotFound, "File not found")
}
pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub trait Metadata: Send + Sync + Clone + 'static {
    fn reserve(&self, file: &FileRecord) -> impl Future<Output = Result<()>> + Send;
    fn activate(
        &self,
        id: &str,
        owner: &str,
        size: i64,
        sha256: &str,
    ) -> impl Future<Output = Result<()>> + Send;
    fn find(
        &self,
        id: &str,
        owner: &str,
    ) -> impl Future<Output = Result<Option<FileRecord>>> + Send;
    fn list(
        &self,
        owner: &str,
        after: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<FileRecord>>> + Send;
    fn deleting(&self, id: &str, owner: &str) -> impl Future<Output = Result<bool>> + Send;
    fn cancel_pending(&self, id: &str, owner: &str) -> impl Future<Output = Result<bool>> + Send;
    fn retired(&self, owner: &str) -> impl Future<Output = Result<Vec<(String, Retired)>>> + Send;
    fn finish(
        &self,
        id: &str,
        owner: &str,
        state: Retired,
    ) -> impl Future<Output = Result<()>> + Send;
}
pub trait Blobs: Send + Sync + Clone + 'static {
    fn create(&self, id: &str) -> impl Future<Output = Result<Writer>> + Send;
    fn read(&self, id: &str) -> impl Future<Output = Result<Reader>> + Send;
    fn commit(&self, id: &str) -> impl Future<Output = Result<()>> + Send;
    fn remove(&self, id: &str) -> impl Future<Output = Result<()>> + Send;
}
async fn abandon<M: Metadata, B: Blobs>(
    metadata: &M,
    blobs: &B,
    id: &str,
    owner: &str,
) -> Result<()> {
    if metadata.cancel_pending(id, owner).await? {
        blobs.remove(id).await?;
        metadata.finish(id, owner, Retired::Deleting).await?;
    }
    Ok(())
}
struct UploadGuard<M: Metadata, B: Blobs>(Option<(M, B, String, String)>);
impl<M: Metadata, B: Blobs> Drop for UploadGuard<M, B> {
    fn drop(&mut self) {
        if let Some((metadata, blobs, id, owner)) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                if abandon(&metadata, &blobs, &id, &owner).await.is_err() {
                    eprintln!("File cleanup deferred; recover interrupted uploads offline");
                }
            });
        }
    }
}
#[derive(Clone)]
pub struct Files<M, B> {
    pub metadata: M,
    pub blobs: B,
    pub max_bytes: u64,
}
impl<M: Metadata, B: Blobs> Files<M, B> {
    pub fn new(metadata: M, blobs: B, max_bytes: u64) -> Result<Self> {
        if max_bytes == 0 || max_bytes > 1024 * 1024 * 1024 {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid upload limit",
            ));
        }
        Ok(Self {
            metadata,
            blobs,
            max_bytes,
        })
    }
    pub async fn upload<R: AsyncRead + Unpin + Send>(
        &self,
        principal: Option<&Principal>,
        name: &str,
        mut input: R,
    ) -> Result<FileRecord> {
        let owner = &require_principal(principal)?.subject;
        if name.is_empty()
            || name.len() > 255
            || name
                .bytes()
                .any(|b| b < 32 || b == 127 || b == b'/' || b == b'\\')
        {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid display filename",
            ));
        }
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(unavailable)?;
        let mut file = FileRecord {
            id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            owner: owner.clone(),
            name: name.into(),
            size: 0,
            sha256: String::new(),
            created_ms: i64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(unavailable)?
                    .as_millis(),
            )
            .map_err(unavailable)?,
        };
        self.metadata.reserve(&file).await?;
        let mut guard = UploadGuard(Some((
            self.metadata.clone(),
            self.blobs.clone(),
            file.id.clone(),
            owner.clone(),
        )));
        let result = async {
            let mut output = self.blobs.create(&file.id).await?;
            let mut hash = Sha256::new();
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let count = input.read(&mut buffer).await.map_err(unavailable)?;
                if count == 0 {
                    break;
                }
                file.size += i64::try_from(count).map_err(unavailable)?;
                if file.size as u64 > self.max_bytes {
                    return Err(ActionError::new(
                        ErrorCode::InvalidInput,
                        "Upload exceeds configured byte limit",
                    ));
                }
                hash.update(&buffer[..count]);
                output
                    .write_all(&buffer[..count])
                    .await
                    .map_err(unavailable)?;
            }
            output.shutdown().await.map_err(unavailable)?;
            drop(output);
            self.blobs.commit(&file.id).await?;
            file.sha256 = format!("{:x}", hash.finalize());
            self.metadata
                .activate(&file.id, owner, file.size, &file.sha256)
                .await
        }
        .await;
        if let Err(error) = result {
            // Leave a pending record if compensation fails; it never becomes visible as a ready file.
            abandon(&self.metadata, &self.blobs, &file.id, owner).await?;
            guard.0 = None;
            return Err(error);
        }
        guard.0 = None;
        Ok(file)
    }
    pub async fn stat(&self, principal: Option<&Principal>, id: &str) -> Result<FileRecord> {
        let owner = &require_principal(principal)?.subject;
        if !valid_id(id) {
            return Err(missing());
        }
        self.metadata.find(id, owner).await?.ok_or_else(missing)
    }
    pub async fn list(
        &self,
        principal: Option<&Principal>,
        after: &str,
        limit: u32,
    ) -> Result<Vec<FileRecord>> {
        let owner = &require_principal(principal)?.subject;
        if !after.is_empty() && !valid_id(after) || !(1..=100).contains(&limit) {
            return Err(ActionError::new(ErrorCode::InvalidInput, "Invalid page"));
        }
        self.metadata.list(owner, after, limit).await
    }
    pub async fn download(
        &self,
        principal: Option<&Principal>,
        id: &str,
    ) -> Result<(FileRecord, Reader)> {
        let file = self.stat(principal, id).await?;
        Ok((file, self.blobs.read(id).await?))
    }
    /// Offline maintenance: all writers must be stopped before recovering pending data.
    pub async fn recover_stopped_uploads(&self, principal: Option<&Principal>) -> Result<usize> {
        let owner = &require_principal(principal)?.subject;
        let mut recovered = 0;
        for (id, state) in self.metadata.retired(owner).await? {
            self.blobs.remove(&id).await?;
            self.metadata.finish(&id, owner, state).await?;
            recovered += 1;
        }
        Ok(recovered)
    }
    pub async fn delete(&self, principal: Option<&Principal>, id: &str) -> Result<()> {
        let owner = &require_principal(principal)?.subject;
        if !valid_id(id) || !self.metadata.deleting(id, owner).await? {
            return Err(missing());
        }
        self.blobs.remove(id).await?;
        self.metadata.finish(id, owner, Retired::Deleting).await
    }
}
