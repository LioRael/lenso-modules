use lenso_app::{ErrorCode, Principal};
use lenso_auth_source::{
    file_local::LocalBlobs,
    file_sqlite::SqliteFiles,
    files::{Blobs, Files, Metadata, Reader, Result, Writer},
};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::io::AsyncReadExt;
#[derive(Clone)]
struct FailingRemove {
    inner: LocalBlobs,
    fail: Arc<AtomicBool>,
}
impl Blobs for FailingRemove {
    async fn create(&self, id: &str) -> Result<Writer> {
        self.inner.create(id).await
    }
    async fn read(&self, id: &str) -> Result<Reader> {
        self.inner.read(id).await
    }
    async fn commit(&self, id: &str) -> Result<()> {
        self.inner.commit(id).await
    }
    async fn remove(&self, id: &str) -> Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(lenso_auth_source::files::unavailable("simulated failure"));
        }
        self.inner.remove(id).await
    }
}
#[tokio::test]
async fn scoped_native_streams_limits_and_retriable_delete() {
    let root = tempfile::tempdir().unwrap();
    let metadata = SqliteFiles::open(&root.path().join("files.sqlite"), "files").unwrap();
    metadata.migrate().await.unwrap();
    let blobs = FailingRemove {
        inner: LocalBlobs::open(root.path(), "files").unwrap(),
        fail: Arc::new(AtomicBool::new(false)),
    };
    let files = Files::new(metadata, blobs, 20).unwrap();
    let owner = Principal {
        subject: "alice".into(),
        scopes: vec![],
    };
    let other = Principal {
        subject: "bob".into(),
        scopes: vec![],
    };
    assert_eq!(
        files
            .upload(None, "none", Cursor::new(b"private"))
            .await
            .unwrap_err()
            .code(),
        ErrorCode::Unauthenticated
    );
    let file = files
        .upload(Some(&owner), "résumé.bin", Cursor::new(b"firstsecond"))
        .await
        .unwrap();
    assert_eq!(
        files.stat(Some(&other), &file.id).await.unwrap_err().code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        files
            .delete(Some(&other), &file.id)
            .await
            .unwrap_err()
            .code(),
        ErrorCode::NotFound
    );
    let (_, mut reader) = files.download(Some(&owner), &file.id).await.unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"firstsecond");
    assert_eq!(
        files
            .upload(Some(&owner), "large", Cursor::new(vec![0; 21]))
            .await
            .unwrap_err()
            .code(),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("files"))
            .unwrap()
            .count(),
        1
    );
    files.blobs.fail.store(true, Ordering::Relaxed);
    assert!(files.delete(Some(&owner), &file.id).await.is_err());
    assert!(files.list(Some(&owner), "", 20).await.unwrap().is_empty());
    files.blobs.fail.store(false, Ordering::Relaxed);
    files.delete(Some(&owner), &file.id).await.unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("files"))
            .unwrap()
            .count(),
        0
    );
    let interrupted = lenso_auth_source::files::FileRecord {
        id: "b".repeat(32),
        owner: "alice".into(),
        name: "interrupted".into(),
        size: 0,
        sha256: String::new(),
        created_ms: 0,
    };
    files.metadata.reserve(&interrupted).await.unwrap();
    let mut writer = files.blobs.create(&interrupted.id).await.unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut writer, b"partial")
        .await
        .unwrap();
    drop(writer);
    assert_eq!(
        files.recover_stopped_uploads(Some(&owner)).await.unwrap(),
        1
    );
    assert!(
        files
            .metadata
            .find(&file.id, "alice")
            .await
            .unwrap()
            .is_none()
    );
}
#[cfg(unix)]
#[tokio::test]
async fn directory_capability_refuses_outside_symlink_reads() {
    let root = tempfile::tempdir().unwrap();
    let blobs = LocalBlobs::open(root.path(), "files").unwrap();
    std::fs::write(root.path().join("secret"), b"outside").unwrap();
    let id = "a".repeat(32);
    std::os::unix::fs::symlink(
        root.path().join("secret"),
        root.path().join("files").join(&id),
    )
    .unwrap();
    assert!(blobs.read(&id).await.is_err());
    assert!(blobs.read("../secret").await.is_err());
}

#[tokio::test]
async fn cancelling_upload_compensates_its_pending_content() {
    let root = tempfile::tempdir().unwrap();
    let metadata = SqliteFiles::open(&root.path().join("files.sqlite"), "files").unwrap();
    metadata.migrate().await.unwrap();
    let blobs = LocalBlobs::open(root.path(), "files").unwrap();
    let files = Arc::new(Files::new(metadata, blobs, 1024).unwrap());
    let (mut sender, receiver) = tokio::io::duplex(1024);
    let upload = files.clone();
    let task = tokio::spawn(async move {
        let principal = Principal {
            subject: "alice".into(),
            scopes: vec![],
        };
        upload
            .upload(Some(&principal), "interrupted", receiver)
            .await
    });
    tokio::io::AsyncWriteExt::write_all(&mut sender, b"partial")
        .await
        .unwrap();
    for _ in 0..100 {
        if std::fs::read_dir(root.path().join("files"))
            .unwrap()
            .count()
            > 0
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(files.metadata.retired("alice").await.unwrap().len(), 1);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(sender);
    for _ in 0..100 {
        if files.metadata.retired("alice").await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(files.metadata.retired("alice").await.unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(root.path().join("files"))
            .unwrap()
            .count(),
        0
    );
}
