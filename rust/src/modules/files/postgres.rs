//! PostgreSQL owner-scoped file metadata with serialized migrations.
use super::files::{FileRecord, Metadata, Result, Retired, unavailable};
use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_postgres::{Client, Config, Row};
struct Connection {
    client: Mutex<Client>,
    driver: JoinHandle<()>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
#[derive(Clone)]
pub struct PostgresFiles {
    db: Arc<Connection>,
    realm: String,
}
fn record(row: Row) -> FileRecord {
    FileRecord {
        id: row.get(0),
        owner: row.get(1),
        name: row.get(2),
        size: row.get(3),
        sha256: row.get(4),
        created_ms: row.get(5),
    }
}
impl PostgresFiles {
    pub async fn connect(url: &str, realm: &str) -> Result<Self> {
        let mut config: Config = url.parse().map_err(unavailable)?;
        config.connect_timeout(std::time::Duration::from_secs(5));
        let tls = postgres_native_tls::MakeTlsConnector::new(
            native_tls::TlsConnector::new().map_err(unavailable)?,
        );
        let (client, connection) = config.connect(tls).await.map_err(unavailable)?;
        let driver = tokio::spawn(async move {
            let _result = connection.await;
        });
        Ok(Self {
            db: Arc::new(Connection {
                client: Mutex::new(client),
                driver,
            }),
            realm: realm.into(),
        })
    }
    pub async fn migrate(&self) -> Result<()> {
        let mut client = self.db.client.lock().await;
        let tx = client.transaction().await.map_err(unavailable)?;
        tx.execute("SELECT pg_advisory_xact_lock(1818586739,1718185061)", &[])
            .await
            .map_err(unavailable)?;
        tx.batch_execute(include_str!("postgres.sql"))
            .await
            .map_err(unavailable)?;
        let row = tx
            .query_one("SELECT MAX(version) FROM lenso_files_migrations", &[])
            .await
            .map_err(unavailable)?;
        if row.get::<_, i32>(0) != 1 {
            return Err(unavailable("Unknown schema"));
        }
        tx.commit().await.map_err(unavailable)
    }
    pub async fn check_schema(&self) -> Result<()> {
        let row = self
            .db
            .client
            .lock()
            .await
            .query_one("SELECT MAX(version) FROM lenso_files_migrations", &[])
            .await
            .map_err(unavailable)?;
        if row.get::<_, i32>(0) == 1 {
            Ok(())
        } else {
            Err(unavailable("Run Files migrations"))
        }
    }
}
impl Metadata for PostgresFiles {
    async fn reserve(&self, f: &FileRecord) -> Result<()> {
        self.db.client.lock().await.execute("INSERT INTO lenso_files(realm,id,owner,name,size,sha256,created_ms,state) VALUES($1,$2,$3,$4,0,'',$5,'pending')",&[&self.realm,&f.id,&f.owner,&f.name,&f.created_ms]).await.map_err(unavailable)?;
        Ok(())
    }
    async fn activate(&self, id: &str, owner: &str, size: i64, sha256: &str) -> Result<()> {
        let n=self.db.client.lock().await.execute("UPDATE lenso_files SET state='ready',size=$1,sha256=$2 WHERE realm=$3 AND id=$4 AND owner=$5 AND state='pending'",&[&size,&sha256,&self.realm,&id,&owner]).await.map_err(unavailable)?;
        if n == 1 {
            Ok(())
        } else {
            Err(unavailable("Invalid transition"))
        }
    }
    async fn find(&self, id: &str, owner: &str) -> Result<Option<FileRecord>> {
        Ok(self.db.client.lock().await.query_opt("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=$1 AND id=$2 AND owner=$3 AND state='ready'",&[&self.realm,&id,&owner]).await.map_err(unavailable)?.map(record))
    }
    async fn list(&self, owner: &str, after: &str, limit: u32) -> Result<Vec<FileRecord>> {
        Ok(self.db.client.lock().await.query("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=$1 AND owner=$2 AND state='ready' AND id>$3 ORDER BY id LIMIT $4",&[&self.realm,&owner,&after,&i64::from(limit)]).await.map_err(unavailable)?.into_iter().map(record).collect())
    }
    async fn deleting(&self, id: &str, owner: &str) -> Result<bool> {
        Ok(self.db.client.lock().await.execute("UPDATE lenso_files SET state='deleting' WHERE realm=$1 AND id=$2 AND owner=$3 AND state IN ('ready','deleting')",&[&self.realm,&id,&owner]).await.map_err(unavailable)?==1)
    }
    async fn cancel_pending(&self, id: &str, owner: &str) -> Result<bool> {
        Ok(self.db.client.lock().await.execute("UPDATE lenso_files SET state='deleting' WHERE realm=$1 AND id=$2 AND owner=$3 AND state='pending'",&[&self.realm,&id,&owner]).await.map_err(unavailable)?==1)
    }
    async fn retired(&self, owner: &str) -> Result<Vec<(String, Retired)>> {
        Ok(self.db.client.lock().await.query("SELECT id,state FROM lenso_files WHERE realm=$1 AND owner=$2 AND state IN ('pending','deleting') LIMIT 100",&[&self.realm,&owner]).await.map_err(unavailable)?.into_iter().map(|r|{let state:String=r.get(1);(r.get(0),if state=="pending"{Retired::Pending}else{Retired::Deleting})}).collect())
    }
    async fn finish(&self, id: &str, owner: &str, state: Retired) -> Result<()> {
        self.db
            .client
            .lock()
            .await
            .execute(
                "DELETE FROM lenso_files WHERE realm=$1 AND id=$2 AND owner=$3 AND state=$4",
                &[&self.realm, &id, &owner, &state.as_str()],
            )
            .await
            .map_err(unavailable)?;
        Ok(())
    }
}
