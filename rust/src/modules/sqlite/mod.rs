//! Realm-isolated SQLite storage, with explicit schema migration.
use super::auth::{AuthStore, Credential, User, internal, random_hex, valid_instance_id};
use lenso_app::{ActionError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Clone)]
pub struct SqliteAuthStore {
    db: Arc<Mutex<Connection>>,
    realm: String,
}
impl SqliteAuthStore {
    pub fn open(filename: &Path, realm: &str) -> Result<Self, ActionError> {
        if !valid_instance_id(realm) {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid SQLite realm",
            ));
        }
        let db = Connection::open(filename).map_err(internal)?;
        db.busy_timeout(Duration::from_secs(5)).map_err(internal)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .map_err(internal)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            realm: realm.into(),
        })
    }
    async fn with_db<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, ActionError> + Send + 'static,
    ) -> Result<T, ActionError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = db.lock().map_err(|_| {
                ActionError::new(ErrorCode::Internal, "Authentication store unavailable")
            })?;
            operation(&mut guard)
        })
        .await
        .map_err(internal)?
    }
    pub async fn migrate(&self) -> Result<(), ActionError> {
        self.with_db(|db| {
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(internal)?;
            tx.execute_batch(include_str!("schema.sql"))
                .map_err(internal)?;
            let version: i64 = tx
                .query_row("SELECT MAX(version) FROM lenso_auth_migrations", [], |r| {
                    r.get(0)
                })
                .map_err(internal)?;
            if version != 1 {
                return Err(ActionError::new(
                    ErrorCode::Internal,
                    "Unsupported authentication schema version",
                ));
            }
            tx.commit().map_err(internal)
        })
        .await
    }
    pub async fn check_schema(&self) -> Result<(), ActionError> {
        self.with_db(|db| {
            let version: i64 = db
                .query_row("SELECT MAX(version) FROM lenso_auth_migrations", [], |r| {
                    r.get(0)
                })
                .map_err(internal)?;
            if version != 1 {
                return Err(ActionError::new(
                    ErrorCode::Internal,
                    "Run the authentication migration first",
                ));
            }
            Ok(())
        })
        .await
    }
}
impl AuthStore for SqliteAuthStore {
    async fn create_user(&self, name: String, password_hash: String) -> Result<User, ActionError> {
        let realm = self.realm.clone();
        self.with_db(move|db|{
            let id=random_hex(16)?;
            if db.execute("INSERT INTO lenso_auth_users(realm,id,name,password_hash) VALUES(?,?,?,?) ON CONFLICT(realm,name) DO NOTHING",params![realm,id,name,password_hash]).map_err(internal)?==0{return Err(ActionError::new(ErrorCode::Conflict,"User already exists"));}
            Ok(User{id,name})
        }).await
    }
    async fn credential(&self, name: &str) -> Result<Option<Credential>, ActionError> {
        let (realm, name) = (self.realm.clone(), name.to_owned());
        self.with_db(move |db| {
            db.query_row(
                "SELECT id,name,password_hash FROM lenso_auth_users WHERE realm=? AND name=?",
                params![realm, name],
                |r| {
                    Ok(Credential {
                        user: User {
                            id: r.get(0)?,
                            name: r.get(1)?,
                        },
                        password_hash: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(internal)
        })
        .await
    }
    async fn user(&self, id: &str) -> Result<Option<User>, ActionError> {
        let (realm, id) = (self.realm.clone(), id.to_owned());
        self.with_db(move |db| {
            db.query_row(
                "SELECT id,name FROM lenso_auth_users WHERE realm=? AND id=?",
                params![realm, id],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(internal)
        })
        .await
    }
    async fn save_session(
        &self,
        hash: String,
        subject: String,
        expires_ms: i64,
    ) -> Result<(), ActionError> {
        let realm = self.realm.clone();
        self.with_db(move|db|{
        db.execute("INSERT INTO lenso_auth_sessions(realm,token_hash,subject,expires_ms) VALUES(?,?,?,?)",params![realm,hash,subject,expires_ms]).map_err(internal)?;Ok(())
    }).await
    }
    async fn session(&self, hash: &str, now: i64) -> Result<Option<User>, ActionError> {
        let (realm, hash) = (self.realm.clone(), hash.to_owned());
        self.with_db(move|db|{
        db.query_row("SELECT u.id,u.name FROM lenso_auth_sessions s JOIN lenso_auth_users u ON u.realm=s.realm AND u.id=s.subject WHERE s.realm=? AND s.token_hash=? AND s.expires_ms>?",params![realm,hash,now],|r|Ok(User{id:r.get(0)?,name:r.get(1)?})).optional().map_err(internal)
    }).await
    }
    async fn revoke_session(&self, hash: &str, subject: &str) -> Result<(), ActionError> {
        let (realm, hash, subject) = (self.realm.clone(), hash.to_owned(), subject.to_owned());
        self.with_db(move |db| {
            db.execute(
                "DELETE FROM lenso_auth_sessions WHERE realm=? AND token_hash=? AND subject=?",
                params![realm, hash, subject],
            )
            .map_err(internal)?;
            Ok(())
        })
        .await
    }
}
