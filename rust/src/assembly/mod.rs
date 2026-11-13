//! Explicit resource and Action assembly for the reference consumer.
#[path = "../modules/auth/mod.rs"]
pub mod auth;
#[path = "../modules/scrypt/mod.rs"]
pub mod passwords;
#[path = "../modules/postgres/mod.rs"]
pub mod postgres;
#[path = "../modules/sqlite/mod.rs"]
pub mod sqlite;
use auth::{Actions, Auth};
use lenso_app::ActionError;
use passwords::ScryptHasher;
use sqlite::SqliteAuthStore;
use std::path::Path;
pub type AuthResources = Auth<SqliteAuthStore, ScryptHasher>;
#[derive(Clone)]
pub struct AuthApplication {
    pub resources: Auth<SqliteAuthStore, ScryptHasher>,
    pub actions: Actions,
    pub store: SqliteAuthStore,
}
pub fn create_auth(filename: &Path, realm: &str) -> Result<AuthApplication, ActionError> {
    let store = SqliteAuthStore::open(filename, realm)?;
    Ok(AuthApplication {
        resources: Auth::new(store.clone(), ScryptHasher::default(), 86_400_000)?,
        actions: Actions::new(realm)?,
        store,
    })
}
