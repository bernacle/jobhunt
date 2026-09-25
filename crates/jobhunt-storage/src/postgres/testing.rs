//! Real Postgres databases for tests (not part of the product).
//!
//! Tests that need Postgres call [`TestDatabase::create`]. It connects to
//! `JOBHUNT_TEST_DATABASE_URL` (a server where the user may create
//! databases), creates a fresh database with a random name, and returns
//! `None` when the variable is unset, so `cargo test` stays runnable
//! without a database server. CI's cloud job sets the variable and
//! `JOBHUNT_REQUIRE_POSTGRES=1`, which turns a missing database into a
//! failure instead of a skip.

use std::str::FromStr;

use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection};

use super::{Keyring, PgSettings, PgStore};

pub const DATABASE_URL_VAR: &str = "JOBHUNT_TEST_DATABASE_URL";
pub const REQUIRE_VAR: &str = "JOBHUNT_REQUIRE_POSTGRES";

/// A database created for one test.
#[derive(Debug, Clone)]
pub struct TestDatabase {
    pub name: String,
    admin: PgConnectOptions,
}

fn admin_options() -> Option<PgConnectOptions> {
    let url = std::env::var(DATABASE_URL_VAR)
        .ok()
        .filter(|u| !u.trim().is_empty());
    match url {
        Some(url) => Some(
            PgConnectOptions::from_str(&url)
                .unwrap_or_else(|e| panic!("{DATABASE_URL_VAR} is not a Postgres URL: {e}")),
        ),
        None => {
            assert!(
                std::env::var(REQUIRE_VAR).is_err(),
                "{REQUIRE_VAR} is set but {DATABASE_URL_VAR} is not"
            );
            eprintln!("skipping: {DATABASE_URL_VAR} is not set (no Postgres for this test)");
            None
        }
    }
}

impl TestDatabase {
    /// A new empty database, or `None` without a test server.
    pub async fn create() -> Option<Self> {
        let admin = admin_options()?;
        let mut bytes = [0u8; 8];
        aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut aes_gcm::aead::OsRng, &mut bytes);
        let name = format!(
            "jobhunt_test_{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let mut conn = PgConnection::connect_with(&admin)
            .await
            .unwrap_or_else(|e| panic!("cannot reach the test Postgres: {e}"));
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&mut conn)
            .await
            .unwrap_or_else(|e| panic!("cannot create a test database: {e}"));
        let _ = conn.close().await;
        Some(Self { name, admin })
    }

    /// Connection options for the new database.
    pub fn options(&self) -> PgConnectOptions {
        self.admin.clone().database(&self.name)
    }

    /// `postgres://…/<name>` for the new database.
    pub fn url(&self) -> String {
        let base = std::env::var(DATABASE_URL_VAR).unwrap_or_default();
        match base.rsplit_once('/') {
            Some((prefix, rest)) => {
                let query = rest.split_once('?').map(|(_, q)| format!("?{q}"));
                format!("{prefix}/{}{}", self.name, query.unwrap_or_default())
            }
            None => base,
        }
    }

    /// A migrated store on the new database.
    pub async fn store(&self, keys: Keyring) -> PgStore {
        let store = PgStore::connect_with(self.options(), &PgSettings::default(), keys)
            .await
            .unwrap_or_else(|e| panic!("cannot connect to the test database: {e}"));
        store
            .migrate()
            .await
            .unwrap_or_else(|e| panic!("migrations failed: {e:?}"));
        store
    }

    /// Drops the database (tests call it when they pass; a failed test
    /// leaves its database for inspection).
    pub async fn drop_database(self) {
        if let Ok(mut conn) = PgConnection::connect_with(&self.admin).await {
            let _ = sqlx::query(&format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.name
            ))
            .execute(&mut conn)
            .await;
            let _ = conn.close().await;
        }
    }
}
