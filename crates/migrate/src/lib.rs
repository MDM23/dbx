use std::{fs, path::Path, sync::LazyLock};

use esql_core::{
    Dialect, Esql, EsqlDriver, FromRow, MigrationError, MigrationErrorKind, Trusted,
    split_statements,
};
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

    /// Apply every migration that has not been applied yet.
    ///
    /// Concurrent boots are serialised by a session lock, so `db` has to be
    /// something that holds one connection for the whole call: a client or a
    /// transaction, not a pool that hands out a different connection per
    /// statement.
    #[cfg_attr(feature = "tracing", tracing::instrument(name = "migrate", skip_all))]
    pub async fn run<D>(self, db: &mut EsqlDriver<'_, D>) -> Result<(), esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        db.execute(<D::Dialect as Dialect>::LOCK).await?;

        let applied = self.apply(db).await;
        let unlocked = db.execute(<D::Dialect as Dialect>::UNLOCK).await;

        applied?;
        unlocked?;

        Ok(())
    }

    async fn apply<D>(&self, db: &mut EsqlDriver<'_, D>) -> Result<(), esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        Self::ensure_table(db).await?;

        let current = Self::get_applied_migrations(db).await?;

        #[cfg(feature = "tracing")]
        match self
            .migrations
            .iter()
            .filter(|m| !current.iter().any(|a| a.version == m.version))
            .count()
        {
            0 => tracing::info!("nothing to migrate"),
            pending => tracing::info!(pending, "running {pending} pending migrations"),
        }

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

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(
            name = "migration",
            skip_all,
            fields(version = migration.version, name = migration.name),
        )
    )]
    async fn apply_migration<D>(
        db: &mut EsqlDriver<'_, D>,
        migration: &Migration,
    ) -> Result<(), esql_core::Error<D::Error>>
    where
        D: Esql,
    {
        #[cfg(feature = "tracing")]
        tracing::info!("running migration {} {}", migration.version, migration.name);

        for statement in split_statements(&migration.sql) {
            db.execute(Trusted::unchecked(statement)).await?;
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
