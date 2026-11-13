//! Realm-isolated PostgreSQL storage with explicit, serialized migrations.
use super::auth::{AuthStore, Credential, User, internal, random_hex, valid_instance_id};
use lenso_app::{ActionError, ErrorCode};
use postgres_native_tls::MakeTlsConnector;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_postgres::{Client, Config, Row};

struct Connection {
    client: Mutex<Client>,
    driver: Option<JoinHandle<()>>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(driver) = &self.driver {
            driver.abort();
        }
    }
}
#[derive(Clone)]
pub struct PostgresAuthStore {
    db: Arc<Connection>,
    realm: String,
}
impl PostgresAuthStore {
    /// Connect using native certificate validation. URL sslmode controls TLS requirements.
    pub async fn connect(url: &str, realm: &str) -> Result<Self, ActionError> {
        if !valid_instance_id(realm) || url.is_empty() {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid PostgreSQL configuration",
            ));
        }
        let mut config: Config = url.parse().map_err(internal)?;
        config.connect_timeout(Duration::from_secs(5));
        let tls = MakeTlsConnector::new(native_tls::TlsConnector::new().map_err(internal)?);
        let (client, connection) = config.connect(tls).await.map_err(internal)?;
        let driver = tokio::spawn(async move {
            // Query failures are reported through Client; retain ownership until the last store drops.
            let _result = connection.await;
        });
        Ok(Self {
            db: Arc::new(Connection {
                client: Mutex::new(client),
                driver: Some(driver),
            }),
            realm: realm.into(),
        })
    }
    /// The caller owns the connection driver and its TLS configuration.
    pub fn from_client(client: Client, realm: &str) -> Result<Self, ActionError> {
        if !valid_instance_id(realm) {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid PostgreSQL realm",
            ));
        }
        Ok(Self {
            db: Arc::new(Connection {
                client: Mutex::new(client),
                driver: None,
            }),
            realm: realm.into(),
        })
    }
    pub async fn migrate(&self) -> Result<(), ActionError> {
        let mut client = self.db.client.lock().await;
        let tx = client.transaction().await.map_err(internal)?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(1818586739, 1635087464)")
            .await
            .map_err(internal)?;
        tx.batch_execute(include_str!("schema.sql"))
            .await
            .map_err(internal)?;
        let version: Option<i32> = tx
            .query_one("SELECT MAX(version) FROM lenso_auth_migrations", &[])
            .await
            .map_err(internal)?
            .get(0);
        if version != Some(1) {
            return Err(ActionError::new(
                ErrorCode::Internal,
                "Unsupported authentication schema version",
            ));
        }
        tx.commit().await.map_err(internal)
    }
    pub async fn check_schema(&self) -> Result<(), ActionError> {
        let version: Option<i32> = self
            .db
            .client
            .lock()
            .await
            .query_one("SELECT MAX(version) FROM lenso_auth_migrations", &[])
            .await
            .map_err(internal)?
            .get(0);
        if version != Some(1) {
            return Err(ActionError::new(
                ErrorCode::Internal,
                "Run the authentication migration first",
            ));
        }
        Ok(())
    }
}
fn user(row: Row) -> User {
    User {
        id: row.get(0),
        name: row.get(1),
    }
}
impl AuthStore for PostgresAuthStore {
    async fn create_user(&self, name: String, password_hash: String) -> Result<User, ActionError> {
        let id = random_hex(16)?;
        let row = self.db.client.lock().await.query_opt("INSERT INTO lenso_auth_users(realm,id,name,password_hash) VALUES($1,$2,$3,$4) ON CONFLICT(realm,name) DO NOTHING RETURNING id,name", &[&self.realm,&id,&name,&password_hash]).await.map_err(internal)?;
        row.map(user)
            .ok_or_else(|| ActionError::new(ErrorCode::Conflict, "User already exists"))
    }
    async fn credential(&self, name: &str) -> Result<Option<Credential>, ActionError> {
        Ok(self
            .db
            .client
            .lock()
            .await
            .query_opt(
                "SELECT id,name,password_hash FROM lenso_auth_users WHERE realm=$1 AND name=$2",
                &[&self.realm, &name],
            )
            .await
            .map_err(internal)?
            .map(|row| Credential {
                password_hash: row.get(2),
                user: user(row),
            }))
    }
    async fn user(&self, id: &str) -> Result<Option<User>, ActionError> {
        Ok(self
            .db
            .client
            .lock()
            .await
            .query_opt(
                "SELECT id,name FROM lenso_auth_users WHERE realm=$1 AND id=$2",
                &[&self.realm, &id],
            )
            .await
            .map_err(internal)?
            .map(user))
    }
    async fn save_session(
        &self,
        hash: String,
        subject: String,
        expires_ms: i64,
    ) -> Result<(), ActionError> {
        self.db.client.lock().await.execute("INSERT INTO lenso_auth_sessions(realm,token_hash,subject,expires_ms) VALUES($1,$2,$3,$4)", &[&self.realm,&hash,&subject,&expires_ms]).await.map_err(internal)?;
        Ok(())
    }
    async fn session(&self, hash: &str, now: i64) -> Result<Option<User>, ActionError> {
        Ok(self.db.client.lock().await.query_opt("SELECT u.id,u.name FROM lenso_auth_sessions s JOIN lenso_auth_users u ON u.realm=s.realm AND u.id=s.subject WHERE s.realm=$1 AND s.token_hash=$2 AND s.expires_ms>$3", &[&self.realm,&hash,&now]).await.map_err(internal)?.map(user))
    }
    async fn revoke_session(&self, hash: &str, subject: &str) -> Result<(), ActionError> {
        self.db
            .client
            .lock()
            .await
            .execute(
                "DELETE FROM lenso_auth_sessions WHERE realm=$1 AND token_hash=$2 AND subject=$3",
                &[&self.realm, &hash, &subject],
            )
            .await
            .map_err(internal)?;
        Ok(())
    }
}
