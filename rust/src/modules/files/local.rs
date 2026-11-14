//! Local objects addressed only by generated keys under an open directory capability.
use super::files::{Blobs, Reader, Result, Writer, unavailable, valid_id};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use std::{path::Path, sync::Arc};
#[derive(Clone)]
pub struct LocalBlobs {
    root: Arc<Dir>,
}
impl LocalBlobs {
    pub fn open(directory: &Path, realm: &str) -> Result<Self> {
        if realm.is_empty()
            || realm.len() > 64
            || !realm
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(unavailable("Invalid realm"));
        }
        let path = directory.join(realm);
        std::fs::create_dir_all(&path).map_err(unavailable)?;
        if std::fs::symlink_metadata(&path)
            .map_err(unavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(unavailable("Symlink root"));
        }
        Ok(Self {
            root: Arc::new(Dir::open_ambient_dir(path, ambient_authority()).map_err(unavailable)?),
        })
    }
    fn key(&self, id: &str) -> Result<()> {
        if valid_id(id) {
            Ok(())
        } else {
            Err(unavailable("Invalid key"))
        }
    }
}
impl Blobs for LocalBlobs {
    async fn create(&self, id: &str) -> Result<Writer> {
        self.key(id)?;
        let file = self
            .root
            .open_with(id, OpenOptions::new().write(true).create_new(true))
            .map_err(unavailable)?;
        Ok(Box::new(tokio::fs::File::from_std(file.into_std())))
    }
    async fn read(&self, id: &str) -> Result<Reader> {
        self.key(id)?;
        if self
            .root
            .symlink_metadata(id)
            .map_err(unavailable)?
            .file_type()
            .is_symlink()
        {
            return Err(unavailable("Symlink object"));
        }
        let file = self.root.open(id).map_err(unavailable)?;
        if !file.metadata().map_err(unavailable)?.is_file() {
            return Err(unavailable("Invalid object"));
        }
        Ok(Box::new(tokio::fs::File::from_std(file.into_std())))
    }
    async fn commit(&self, id: &str) -> Result<()> {
        self.key(id)?;
        self.root
            .open(id)
            .map_err(unavailable)?
            .sync_all()
            .map_err(unavailable)
    }
    async fn remove(&self, id: &str) -> Result<()> {
        self.key(id)?;
        match self.root.remove_file(id) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(unavailable(e)),
        }
    }
}
