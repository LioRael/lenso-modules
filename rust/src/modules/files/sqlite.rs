//! SQLite file metadata with owner-scoped lifecycle transitions.
use super::files::{FileRecord, Metadata, Result, Retired, unavailable};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct SqliteFiles {
    db: Arc<Mutex<Connection>>,
    realm: String,
}
fn record(row: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        owner: row.get(1)?,
        name: row.get(2)?,
        size: row.get(3)?,
        sha256: row.get(4)?,
        created_ms: row.get(5)?,
    })
}
impl SqliteFiles {
    pub fn open(path: &Path, realm: &str) -> Result<Self> {
        let db = Connection::open(path).map_err(unavailable)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(unavailable)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(unavailable)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            realm: realm.into(),
        })
    }
    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection, &str) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = self.db.clone();
        let realm = self.realm.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = db.lock().map_err(unavailable)?;
            operation(&mut connection, &realm)
        })
        .await
        .map_err(unavailable)?
    }
    pub async fn migrate(&self) -> Result<()> {
        self.run(|db, _| {
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(unavailable)?;
            tx.execute_batch(include_str!("sqlite.sql"))
                .map_err(unavailable)?;
            let v: i64 = tx
                .query_row("SELECT MAX(version) FROM lenso_files_migrations", [], |r| {
                    r.get(0)
                })
                .map_err(unavailable)?;
            if v != 1 {
                return Err(unavailable("Unknown schema"));
            }
            tx.commit().map_err(unavailable)
        })
        .await
    }
    pub async fn check_schema(&self) -> Result<()> {
        self.run(|db, _| {
            let v: i64 = db
                .query_row("SELECT MAX(version) FROM lenso_files_migrations", [], |r| {
                    r.get(0)
                })
                .map_err(unavailable)?;
            if v == 1 {
                Ok(())
            } else {
                Err(unavailable("Run Files migrations"))
            }
        })
        .await
    }
}
impl Metadata for SqliteFiles {
    async fn reserve(&self, f: &FileRecord) -> Result<()> {
        let f = f.clone();
        self.run(move|db,realm|{db.execute("INSERT INTO lenso_files(realm,id,owner,name,size,sha256,created_ms,state) VALUES(?1,?2,?3,?4,0,'',?5,'pending')",params![realm,f.id,f.owner,f.name,f.created_ms]).map_err(unavailable)?;Ok(())}).await
    }
    async fn activate(&self, id: &str, owner: &str, size: i64, sha256: &str) -> Result<()> {
        let (id, owner, sha256) = (id.to_owned(), owner.to_owned(), sha256.to_owned());
        self.run(move|db,realm|{let n=db.execute("UPDATE lenso_files SET state='ready',size=?1,sha256=?2 WHERE realm=?3 AND id=?4 AND owner=?5 AND state='pending'",params![size,sha256,realm,id,owner]).map_err(unavailable)?;if n==1{Ok(())}else{Err(unavailable("Invalid transition"))}}).await
    }
    async fn find(&self, id: &str, owner: &str) -> Result<Option<FileRecord>> {
        let (id, owner) = (id.to_owned(), owner.to_owned());
        self.run(move|db,realm|db.query_row("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=?1 AND id=?2 AND owner=?3 AND state='ready'",params![realm,id,owner],record).optional().map_err(unavailable)).await
    }
    async fn list(&self, owner: &str, after: &str, limit: u32) -> Result<Vec<FileRecord>> {
        let (owner, after) = (owner.to_owned(), after.to_owned());
        self.run(move|db,realm|{
   let mut stmt=db.prepare("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=?1 AND owner=?2 AND state='ready' AND id>?3 ORDER BY id LIMIT ?4").map_err(unavailable)?;
   stmt.query_map(params![realm,owner,after,limit],record).map_err(unavailable)?.collect::<std::result::Result<Vec<_>,_>>().map_err(unavailable)
  }).await
    }
    async fn deleting(&self, id: &str, owner: &str) -> Result<bool> {
        let (id, owner) = (id.to_owned(), owner.to_owned());
        self.run(move|db,realm|Ok(db.execute("UPDATE lenso_files SET state='deleting' WHERE realm=?1 AND id=?2 AND owner=?3 AND state IN ('ready','deleting')",params![realm,id,owner]).map_err(unavailable)?==1)).await
    }
    async fn cancel_pending(&self, id: &str, owner: &str) -> Result<bool> {
        let (id, owner) = (id.to_owned(), owner.to_owned());
        self.run(move|db,realm|Ok(db.execute("UPDATE lenso_files SET state='deleting' WHERE realm=?1 AND id=?2 AND owner=?3 AND state='pending'",params![realm,id,owner]).map_err(unavailable)?==1)).await
    }
    async fn retired(&self, owner: &str) -> Result<Vec<(String, Retired)>> {
        let owner = owner.to_owned();
        self.run(move|db,realm|{let mut stmt=db.prepare("SELECT id,state FROM lenso_files WHERE realm=?1 AND owner=?2 AND state IN ('pending','deleting') LIMIT 100").map_err(unavailable)?;
   stmt.query_map(params![realm,owner],|row|{let state:String=row.get(1)?;Ok((row.get(0)?,if state=="pending"{Retired::Pending}else{Retired::Deleting}))}).map_err(unavailable)?.collect::<std::result::Result<Vec<_>,_>>().map_err(unavailable)
  }).await
    }
    async fn finish(&self, id: &str, owner: &str, state: Retired) -> Result<()> {
        let (id, owner) = (id.to_owned(), owner.to_owned());
        self.run(move |db, realm| {
            db.execute(
                "DELETE FROM lenso_files WHERE realm=?1 AND id=?2 AND owner=?3 AND state=?4",
                params![realm, id, owner, state.as_str()],
            )
            .map_err(unavailable)?;
            Ok(())
        })
        .await
    }
}
