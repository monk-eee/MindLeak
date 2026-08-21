//! Serialising schema migrations against a Postgres advisory lock.
//!
//! Every store's `connect()` runs its own idempotent `CREATE TABLE IF NOT
//! EXISTS` migration DDL, and every store is constructed independently, so
//! against a genuinely cold database two concurrent `connect()` calls can
//! both see "the table does not exist yet" and both attempt to create it --
//! `IF NOT EXISTS` guards a single statement, not a race between two
//! sessions each deciding to run it at the same time. Measured 2026-08-17:
//! 14 of 92 tests failed this way on a fresh `docker compose down -v`
//! container, all on the same underlying `pg_type` catalog collision
//! (gaps.d/ackplane-server-schema-migration-races-on-a-cold-database.md).
//!
//! [`pg_advisory_xact_lock`] blocks until the lock is free, and releases it
//! automatically at transaction end (commit, rollback, or a dropped
//! connection) -- no explicit unlock call, and no lock left behind by a
//! panicking caller.
//!
//! [`pg_advisory_xact_lock`]: https://www.postgresql.org/docs/current/functions-admin.html#FUNCTIONS-ADVISORY-LOCKS

use tokio_postgres::Client;

/// One key per migration *file*, not per store: `fleet.rs` re-applies the
/// same `0001`/`0002`/`0003` migrations the ledger/projection/enrollment
/// stores each apply on their own, so a lock keyed by store would let two
/// callers migrate the identical schema concurrently anyway. Keying by the
/// migration file number keeps every caller of the same schema on the same
/// lock, and gives a new migration its number as the obvious next key.
pub(crate) mod key {
    /// `migrations/0001_ledger.sql`
    pub(crate) const LEDGER: i64 = 1;
    /// `migrations/0002_projection.sql`
    pub(crate) const PROJECTION: i64 = 2;
    /// `migrations/0003_enrollment.sql`
    pub(crate) const ENROLLMENT: i64 = 3;
    /// `migrations/0004_signing_keys.sql`
    pub(crate) const SIGNING_KEYS: i64 = 4;
    /// `migrations/0005_claim_delegation.sql`
    pub(crate) const CLAIM_DELEGATION: i64 = 5;
    /// `migrations/0006_claim_authentication_nonces.sql`
    pub(crate) const CLAIM_AUTHENTICATION_NONCES: i64 = 6;
    /// `migrations/0007_knowledge.sql`
    pub(crate) const KNOWLEDGE: i64 = 7;
    /// `migrations/0008_knowledge_authentication_nonces.sql`
    pub(crate) const KNOWLEDGE_AUTHENTICATION_NONCES: i64 = 8;
    /// `migrations/0009_constitution.sql`
    pub(crate) const CONSTITUTION: i64 = 9;
    /// `migrations/0010_constitution_authentication_nonces.sql`
    pub(crate) const CONSTITUTION_AUTHENTICATION_NONCES: i64 = 10;
    /// `migrations/0014_evidence.sql`
    pub(crate) const EVIDENCE: i64 = 14;
}

/// Apply `migration_sql` inside a transaction holding the advisory lock
/// named by `lock_key`. A concurrent caller applying the same migration
/// blocks on the lock rather than racing this one, and sees the schema
/// already present (a true no-op) once it is admitted.
pub(crate) async fn migrate_locked(
    client: &mut Client,
    lock_key: i64,
    migration_sql: &str,
) -> Result<(), tokio_postgres::Error> {
    let transaction = client.transaction().await?;
    transaction
        .execute("SELECT pg_advisory_xact_lock($1)", &[&lock_key])
        .await?;
    transaction.batch_execute(migration_sql).await?;
    transaction.commit().await
}

#[cfg(test)]
mod tests {
    use tokio_postgres::NoTls;

    /// Proves the mutual-exclusion primitive itself, deterministically: a
    /// second connection's non-blocking `pg_try_advisory_xact_lock` on the
    /// same key must fail while the first transaction still holds it. A key
    /// no real migration uses, so this can never contend with one actually
    /// running concurrently.
    const TEST_ONLY_LOCK_KEY: i64 = -1;

    #[tokio::test]
    async fn a_second_connection_cannot_acquire_the_same_advisory_lock_while_the_first_holds_it() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (mut holder, holder_connection) =
            tokio_postgres::connect(&database_url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = holder_connection.await;
        });
        let (contender, contender_connection) =
            tokio_postgres::connect(&database_url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = contender_connection.await;
        });

        let holder_txn = holder.transaction().await.unwrap();
        holder_txn
            .execute("SELECT pg_advisory_xact_lock($1)", &[&TEST_ONLY_LOCK_KEY])
            .await
            .unwrap();

        let acquired: bool = contender
            .query_one(
                "SELECT pg_try_advisory_xact_lock($1)",
                &[&TEST_ONLY_LOCK_KEY],
            )
            .await
            .unwrap()
            .get(0);
        assert!(
            !acquired,
            "a second connection must not acquire the same advisory lock while \
             the first transaction still holds it"
        );

        holder_txn.commit().await.unwrap();
    }

    /// Once the holder's transaction ends, the lock is free -- proving
    /// release is automatic and does not need an explicit unlock call.
    #[tokio::test]
    async fn the_lock_is_released_when_the_holding_transaction_commits() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (mut holder, holder_connection) =
            tokio_postgres::connect(&database_url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = holder_connection.await;
        });
        let (contender, contender_connection) =
            tokio_postgres::connect(&database_url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = contender_connection.await;
        });

        let holder_txn = holder.transaction().await.unwrap();
        holder_txn
            .execute(
                "SELECT pg_advisory_xact_lock($1)",
                &[&(TEST_ONLY_LOCK_KEY - 1)],
            )
            .await
            .unwrap();
        holder_txn.commit().await.unwrap();

        let acquired: bool = contender
            .query_one(
                "SELECT pg_try_advisory_xact_lock($1)",
                &[&(TEST_ONLY_LOCK_KEY - 1)],
            )
            .await
            .unwrap()
            .get(0);
        assert!(
            acquired,
            "the lock must be free once the holding transaction has committed"
        );
    }

    /// `migrate_locked` itself: idempotent DDL applied twice in a row (the
    /// shape every `connect()` caller now uses) must succeed both times, not
    /// only the first -- this is the direct regression test for the gap:
    /// unguarded concurrent `CREATE TABLE IF NOT EXISTS` calls could each
    /// see "missing" and race the catalog insert. Run sequentially here
    /// (proving idempotency), with the two tests above proving the
    /// concurrency guard that makes a real race impossible.
    #[tokio::test]
    async fn migrate_locked_is_safe_to_call_twice() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (mut client, connection) = tokio_postgres::connect(&database_url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let ddl = "CREATE TABLE IF NOT EXISTS migrate_locked_smoke_test (id INTEGER PRIMARY KEY)";

        super::migrate_locked(&mut client, TEST_ONLY_LOCK_KEY - 2, ddl)
            .await
            .unwrap();
        super::migrate_locked(&mut client, TEST_ONLY_LOCK_KEY - 2, ddl)
            .await
            .unwrap();
    }
}
