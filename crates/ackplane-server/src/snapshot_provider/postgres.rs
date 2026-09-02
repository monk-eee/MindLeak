//! pg_dump/pg_restore invocation, connection-string handling, and the key and file plumbing they need.

use super::*;

/// Counts `TABLE DATA` entries in the archive's own table of contents,
/// read-only against the decrypted archive file via `pg_restore --list`,
/// mirroring `inspect_snapshot_artifact`'s existing invocation. `None` when
/// `pg_restore --list` itself fails to run or reports a non-zero status,
/// since the reconciliation this feeds has nothing to reconcile against
/// without it.
pub(super) async fn archive_table_data_count(
    pg_restore_path: &str,
    temp_path: &Path,
) -> Option<i64> {
    let output = Command::new(pg_restore_path)
        .arg("--list")
        .arg(temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(" TABLE DATA "))
        .count();
    Some(i64::try_from(count).unwrap_or(i64::MAX))
}

/// Connects to the scratch database and reports the base table count
/// (excluding system schemas) and the total live row count summed across
/// every one of them.
pub(super) async fn reconcile_tables(
    scratch_url: &str,
) -> Result<(Option<i64>, Option<i64>), String> {
    let (client, connection) = tokio_postgres::connect(scratch_url, NoTls)
        .await
        .map_err(|error| format!("could not connect to the scratch database: {error}"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let rows = client
        .query(
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
               AND table_type = 'BASE TABLE'",
            &[],
        )
        .await
        .map_err(|error| format!("could not list restored tables: {error}"))?;

    let table_count = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let mut total_rows: i64 = 0;
    for row in &rows {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        let count_row = client
            .query_one(
                &format!(
                    "SELECT COUNT(*) FROM {}.{}",
                    quote_ident(&schema),
                    quote_ident(&table)
                ),
                &[],
            )
            .await
            .map_err(|error| format!("could not count rows in {schema}.{table}: {error}"))?;
        let rows_in_table: i64 = count_row.get(0);
        total_rows = total_rows.saturating_add(rows_in_table);
    }

    Ok((Some(table_count), Some(total_rows)))
}

/// Double-quotes a Postgres identifier for interpolation into a query,
/// doubling any internal `"` -- `information_schema` names are trusted
/// metadata, not caller input, but this is one line of defence either way.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Splits a `postgresql://[user[:pass]@]host[:port]/dbname[?params]` url into
/// its authority (`user:pass@host:port`) and `dbname` path segment, so the
/// same-database refusal and scratch-url construction below never have to
/// reparse it differently. `None` for a `key=value` connection string, or any
/// url missing a `://` or a path segment.
pub(super) fn split_authority_and_dbname(url: &str) -> Option<(&str, &str)> {
    let base = url.split('?').next().unwrap_or(url);
    let scheme_end = base.find("://")? + 3;
    let rest = base.get(scheme_end..)?;
    let path_start = rest.find('/')?;
    Some((&rest[..path_start], &rest[path_start + 1..]))
}

/// Rebuilds `url` with its `dbname` path segment replaced by `new_dbname`,
/// preserving the authority and any query string. `None` under the same
/// conditions as [`split_authority_and_dbname`].
pub(crate) fn with_dbname(url: &str, new_dbname: &str) -> Option<String> {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let scheme_end = base.find("://")? + 3;
    let rest = base.get(scheme_end..)?;
    let path_start = rest.find('/')?;
    let mut result = format!("{}/{new_dbname}", &base[..scheme_end + path_start]);
    if let Some(query) = query {
        result.push('?');
        result.push_str(query);
    }
    Some(result)
}

pub(crate) fn unique_suffix() -> String {
    let mut bytes = [0_u8; 8];
    let _ = getrandom::getrandom(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) async fn run_pg_dump(
    pg_dump_path: &str,
    database_url: &str,
) -> Result<Vec<u8>, SnapshotProviderError> {
    let output = Command::new(pg_dump_path)
        .arg(database_url)
        .arg("--format=custom")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| SnapshotProviderError::Spawn {
            path: pg_dump_path.to_string(),
            source,
        })?;
    if !output.status.success() {
        return Err(SnapshotProviderError::PgDumpFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(output.stdout)
}

/// Loads the per-installation symmetric key `path` holds, generating and
/// persisting a fresh 32-byte one on first run. Mirrors
/// `ackplane_bridge::load_or_generate_salt`'s exact generate-once-and-persist
/// shape (ADR-0098 decision 3's precedent); duplicated in this crate rather
/// than imported because `ackplane-server` depends on no downstream crate,
/// including `ackplane-bridge` (ADR-0082 clause 1), and this is a ~15-line
/// single-purpose helper, not shared business logic.
pub(super) async fn load_or_generate_key(path: &Path) -> io::Result<[u8; KEY_BYTES]> {
    if let Ok(existing) = fs::read(path).await {
        if existing.len() == KEY_BYTES {
            let mut key = [0_u8; KEY_BYTES];
            key.copy_from_slice(&existing);
            return Ok(key);
        }
    }
    let mut key = [0_u8; KEY_BYTES];
    getrandom::getrandom(&mut key)
        .map_err(|error| io::Error::other(format!("could not generate a snapshot key: {error}")))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    write_atomically(path, &key).await?;
    Ok(key)
}

/// Writes via a sibling temp file and rename so a crash mid-write never
/// leaves a partially written artifact or key at `path`.
pub(super) async fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
    }
    fs::rename(&temp_path, path).await
}

/// Node ids embed a `namespace:hex` request id (see
/// `administration_store::model::hex_id`), and `:` is invalid in a Windows
/// filename (reserved for drive letters) even though it is fine on Linux and
/// macOS -- replaced so an artifact path stays valid on every platform this
/// toolchain runs on.
pub(super) fn filesystem_safe_id(id: &str) -> String {
    id.replace(':', "_")
}
