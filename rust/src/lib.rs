//! Native Auth source consumed through the published Lenso SDK.
#[path = "assembly/mod.rs"]
pub mod application;

#[path = "modules/files/local.rs"]
pub mod file_local;
#[path = "modules/files/postgres.rs"]
pub mod file_postgres;
#[path = "modules/files/sqlite.rs"]
pub mod file_sqlite;
#[path = "modules/files/mod.rs"]
pub mod files;
