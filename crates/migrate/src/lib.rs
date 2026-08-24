use std::{fs, path::Path, sync::LazyLock};

use esql_core::{Esql, EsqlDriver, FromRow, MigrationError, MigrationErrorKind, Trusted};
use regex::Regex;
use sha2::{Digest as _, Sha256};

#[derive(Debug)]
pub struct Migration {
    pub checksum: String,
    pub name: String,
    pub sql: String,
    pub version: i64,
}

struct AppliedMigration {
    checksum: String,
    version: i64,
}

impl FromRow for AppliedMigration {
    fn from_row<R: esql_core::Row>(row: &R) -> Result<Self, esql_core::FromRowError> {
        Ok(AppliedMigration {
            checksum: row.try_get("checksum")?,
            version: row.try_get("version")?,
        })
    }
}

pub struct Migrator {
    pub migrations: Vec<Migration>,
}

static FILENAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<version>[0-9]+)_(?P<name>[a-z_]+)\.sql$").unwrap());

impl TryFrom<&Path> for Migration {
    type Error = MigrationErrorKind;

    fn try_from(entry: &Path) -> Result<Self, Self::Error> {
        let file_name_os = entry.file_name().ok_or(MigrationErrorKind::FilenameError)?;
        let file_name = file_name_os.to_string_lossy();

        let cap = FILENAME_REGEX
            .captures(&file_name)
            .ok_or(MigrationErrorKind::FilenameError)?;

        let name = cap
            .name("name")
            .map(|name| name.as_str())
            .ok_or(MigrationErrorKind::FilenameError)?
            .to_owned();

        let version = cap
            .name("version")
            .map(|version| version.as_str())
            .ok_or(MigrationErrorKind::FilenameError)?
            .parse()?;

        let sql = fs::read_to_string(entry)?;
        let checksum = base16ct::lower::encode_string(&Sha256::digest(sql.as_bytes()));

        Ok(Self {
            checksum,
            name,
            sql,
            version,
        })
    }
}

impl Migration {
    pub fn new(file: impl AsRef<Path>) -> Result<Self, MigrationError> {
        let file = file.as_ref();

        file.try_into().map_err(|e| MigrationError {
            error: e,
            filename: file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        })
    }
}

impl Migrator {
    pub fn new(migrations: Vec<Migration>) -> Self {
        Migrator { migrations }
    }

    pub async fn run<D>(self, db: &mut EsqlDriver<'_, D>) -> Result<(), esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        Self::ensure_table(db).await?;

        let current = Self::get_applied_migrations(db).await?;

        for migration in &self.migrations {
            match current.iter().find(|a| a.version == migration.version) {
                None => {
                    Self::apply_migration(db, migration).await?;
                }
                Some(a) => {
                    if a.checksum != migration.checksum {
                        Err(MigrationError {
                            error: MigrationErrorKind::ChecksumError,
                            filename: migration.name.to_string(),
                        })?;
                    }
                }
            };
        }

        Ok(())
    }

    async fn ensure_table<D>(db: &mut EsqlDriver<'_, D>) -> Result<u64, esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        db.execute(
            r#"
                CREATE TABLE IF NOT EXISTS migrations (
                    version     BIGINT PRIMARY KEY,
                    name        TEXT NOT NULL,
                    checksum    VARCHAR(64),
                    created_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#,
        )
        .await
    }

    async fn get_applied_migrations<D>(
        db: &mut EsqlDriver<'_, D>,
    ) -> Result<Vec<AppliedMigration>, esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        db.query(
            r#"
                SELECT version, checksum
                FROM migrations
                ORDER BY version
            "#,
        )
        .await
    }

    async fn apply_migration<D>(
        db: &mut EsqlDriver<'_, D>,
        migration: &Migration,
    ) -> Result<(), esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        for stmt in migration.sql.split(";") {
            if !stmt.trim().is_empty() {
                db.execute(Trusted::unchecked(stmt)).await?;
            }
        }

        db.execute((
            r#"
                INSERT INTO migrations ( version, name, checksum )
                VALUES (?, ?, ?)
            "#,
            migration.version,
            migration.name.to_string(),
            migration.checksum.to_string(),
        ))
        .await?;

        Ok(())
    }
}
