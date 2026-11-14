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
            return Err(
                ActionError::new(ErrorCode::InvalidInput, "Invalid display filename").with_issues(
                    vec![input_issue("name", lenso_app::InputIssueCode::InvalidValue)],
                ),
            );
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
                    )
                    .with_issues(vec![input_issue(
                        "source",
                        lenso_app::InputIssueCode::OutOfRange,
                    )]));
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
        let mut issues = Vec::new();
        if !after.is_empty() && !valid_id(after) {
            issues.push(input_issue(
                "after",
                lenso_app::InputIssueCode::InvalidValue,
            ));
        }
        if !(1..=100).contains(&limit) {
            issues.push(input_issue("limit", lenso_app::InputIssueCode::OutOfRange));
        }
        if !issues.is_empty() {
            return Err(
                ActionError::new(ErrorCode::InvalidInput, "Invalid page").with_issues(issues)
            );
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

/// Optional SDK handles over native Files methods, including non-serializable streams.
#[derive(Debug, Clone)]
pub struct Actions {
    pub upload: Upload,
    pub list: List,
    pub stat: Stat,
    pub download: Download,
    pub delete: Delete,
}
#[derive(Debug, Clone)]
pub struct Upload(String);
#[derive(Debug, Clone)]
pub struct List(String);
#[derive(Debug, Clone)]
pub struct Stat(String);
#[derive(Debug, Clone)]
pub struct Download(String);
#[derive(Debug, Clone)]
pub struct Delete(String);

/// Owned native stream. The mutex satisfies SDK input Sync without requiring the reader to be Sync.
pub struct UploadInput {
    pub name: String,
    pub source: std::sync::Mutex<Reader>,
}
impl UploadInput {
    pub fn new(name: String, source: Reader) -> Self {
        Self {
            name,
            source: std::sync::Mutex::new(source),
        }
    }
}
#[derive(Debug, Deserialize, Default)]
pub struct Page {
    pub after: Option<String>,
    pub limit: Option<u32>,
}
#[derive(Debug, Deserialize)]
pub struct FileInput {
    pub id: String,
}
fn action_metadata(operation: &str) -> lenso_app::ActionMetadata {
    let declaration: serde_json::Value = serde_json::from_str(include_str!("metadata.json"))
        .expect("embedded Files metadata is valid JSON");
    serde_json::from_value(declaration["actions"][operation].clone())
        .expect("embedded Files Action metadata matches SDK")
}
fn input_issue(field: &str, code: lenso_app::InputIssueCode) -> lenso_app::InputIssue {
    lenso_app::InputIssue {
        path: vec![lenso_app::InputSegment::Field(field.into())],
        code,
    }
}
impl Actions {
    pub fn new(id: &str) -> Result<Self> {
        if id.split('.').count() > 32
            || !id.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 64
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid Files instance identifier",
            ));
        }
        Ok(Self {
            upload: Upload(format!("{id}.upload")),
            list: List(format!("{id}.list")),
            stat: Stat(format!("{id}.stat")),
            download: Download(format!("{id}.download")),
            delete: Delete(format!("{id}.delete")),
        })
    }
    pub fn describe<M: Metadata, B: Blobs>(&self, id: &str) -> lenso_app::Module {
        let declaration: serde_json::Value = serde_json::from_str(include_str!("metadata.json"))
            .expect("embedded Files metadata is valid JSON");
        let mut module = lenso_app::Module::new(id).with_metadata(
            serde_json::from_value(declaration["module"].clone())
                .expect("embedded Files Module metadata matches SDK"),
        );
        module
            .action::<Files<M, B>, _>(&self.upload)
            .action::<Files<M, B>, _>(&self.list)
            .action::<Files<M, B>, _>(&self.stat)
            .action::<Files<M, B>, _>(&self.download)
            .action::<Files<M, B>, _>(&self.delete)
            .resource(format!("{id}.metadata"))
            .resource(format!("{id}.content"));
        module
    }
}
impl<M: Metadata, B: Blobs> lenso_app::Action<Files<M, B>> for Upload {
    type Input = UploadInput;
    type Output = FileRecord;
    fn id(&self) -> &str {
        &self.0
    }
    fn metadata(&self) -> Option<lenso_app::ActionMetadata> {
        Some(action_metadata("upload"))
    }
    async fn authorize(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        _: &Self::Input,
    ) -> Result<()> {
        require_principal(context.principal.as_ref()).map(|_| ())
    }
    async fn run(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        input: Self::Input,
    ) -> Result<FileRecord> {
        context
            .resources
            .upload(
                context.principal.as_ref(),
                &input.name,
                input.source.into_inner().map_err(unavailable)?,
            )
            .await
    }
}
impl<M: Metadata, B: Blobs> lenso_app::Action<Files<M, B>> for List {
    type Input = Page;
    type Output = Vec<FileRecord>;
    fn id(&self) -> &str {
        &self.0
    }
    fn metadata(&self) -> Option<lenso_app::ActionMetadata> {
        Some(action_metadata("list"))
    }
    async fn authorize(&self, context: &lenso_app::Context<Files<M, B>>, _: &Page) -> Result<()> {
        require_principal(context.principal.as_ref()).map(|_| ())
    }
    async fn run(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        input: Page,
    ) -> Result<Self::Output> {
        context
            .resources
            .list(
                context.principal.as_ref(),
                input.after.as_deref().unwrap_or(""),
                input.limit.unwrap_or(20),
            )
            .await
    }
}
impl<M: Metadata, B: Blobs> lenso_app::Action<Files<M, B>> for Stat {
    type Input = FileInput;
    type Output = FileRecord;
    fn id(&self) -> &str {
        &self.0
    }
    fn metadata(&self) -> Option<lenso_app::ActionMetadata> {
        Some(action_metadata("stat"))
    }
    async fn authorize(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        _: &FileInput,
    ) -> Result<()> {
        require_principal(context.principal.as_ref()).map(|_| ())
    }
    async fn run(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        input: FileInput,
    ) -> Result<FileRecord> {
        context
            .resources
            .stat(context.principal.as_ref(), &input.id)
            .await
    }
}
impl<M: Metadata, B: Blobs> lenso_app::Action<Files<M, B>> for Download {
    type Input = FileInput;
    type Output = (FileRecord, Reader);
    fn id(&self) -> &str {
        &self.0
    }
    fn metadata(&self) -> Option<lenso_app::ActionMetadata> {
        Some(action_metadata("download"))
    }
    async fn authorize(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        _: &FileInput,
    ) -> Result<()> {
        require_principal(context.principal.as_ref()).map(|_| ())
    }
    async fn run(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        input: FileInput,
    ) -> Result<Self::Output> {
        context
            .resources
            .download(context.principal.as_ref(), &input.id)
            .await
    }
}
impl<M: Metadata, B: Blobs> lenso_app::Action<Files<M, B>> for Delete {
    type Input = FileInput;
    type Output = ();
    fn id(&self) -> &str {
        &self.0
    }
    fn metadata(&self) -> Option<lenso_app::ActionMetadata> {
        Some(action_metadata("delete"))
    }
    async fn authorize(
        &self,
        context: &lenso_app::Context<Files<M, B>>,
        _: &FileInput,
    ) -> Result<()> {
        require_principal(context.principal.as_ref()).map(|_| ())
    }
    async fn run(&self, context: &lenso_app::Context<Files<M, B>>, input: FileInput) -> Result<()> {
        context
            .resources
            .delete(context.principal.as_ref(), &input.id)
            .await
    }
}
