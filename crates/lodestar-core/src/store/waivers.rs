//! Persistence for bounded constitutional waivers (SPEC-CONSTITUTION §9).

use rusqlite::{params, OptionalExtension, Row};

use crate::error::{LodestarError, Result};
use crate::waiver::{validate_request, Waiver, WaiverRequest, WaiverStatus};

use super::{collect, LodestarStore};

const WAIVER_COLS: &str = "id, clause_id, constitution_version, scope, reason, approved_by, \
     created_at, expires_at, remediation_task_id, status, revoked_by, revoked_at, \
     revocation_reason";

impl LodestarStore {
    /// Grant a scoped, expiring exception to one clause.
    ///
    /// The clause is loaded and checked rather than trusted from the caller:
    /// whether an exception is permitted at all, and who may permit it, are
    /// properties of the policy being excepted, not of the request.
    pub fn grant_waiver(&self, id: &str, request: &WaiverRequest, now: i64) -> Result<Waiver> {
        let clause = self
            .get_goal(&request.clause_id)?
            .ok_or_else(|| LodestarError::NotFound(request.clause_id.clone()))?;
        validate_request(&clause, request, now)?;

        if let Some(task_id) = request.remediation_task_id.as_deref() {
            if self.get_task(task_id)?.is_none() {
                return Err(LodestarError::NotFound(format!(
                    "remediation task {task_id}"
                )));
            }
        }

        let constitution_version = self
            .active_constitution_version()?
            .map(|version| version.id)
            .or(clause.constitution_version.clone());

        self.conn.execute(
            "INSERT INTO waivers
                 (id, clause_id, constitution_version, scope, reason, approved_by,
                  created_at, expires_at, remediation_task_id, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active')",
            params![
                id,
                request.clause_id,
                constitution_version,
                request.scope,
                request.reason,
                request.approved_by,
                now,
                request.expires_at,
                request.remediation_task_id,
            ],
        )?;
        self.waiver(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }

    /// Withdraw a waiver. Attributed, immediate for future checks, and never a
    /// delete — the exception happened, and the record of it survives.
    pub fn revoke_waiver(
        &self,
        id: &str,
        revoked_by: &str,
        reason: &str,
        now: i64,
    ) -> Result<Waiver> {
        if revoked_by.trim().is_empty() || reason.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "revoking a waiver requires an attributed revoker and a reason".to_string(),
            ));
        }
        let changed = self.conn.execute(
            "UPDATE waivers
                SET status = 'revoked', revoked_by = ?2, revoked_at = ?3, revocation_reason = ?4
              WHERE id = ?1 AND status = 'active'",
            params![id, revoked_by, now, reason],
        )?;
        if changed == 0 {
            return match self.waiver(id)? {
                Some(_) => Err(LodestarError::Invalid(format!(
                    "waiver {id} is already revoked"
                ))),
                None => Err(LodestarError::NotFound(id.to_string())),
            };
        }
        self.waiver(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }

    pub fn waiver(&self, id: &str) -> Result<Option<Waiver>> {
        let sql = format!("SELECT {WAIVER_COLS} FROM waivers WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![id], row_to_waiver)
            .optional()?)
    }

    /// Every waiver ever granted against one clause, newest first — including
    /// lapsed and revoked ones, because the audit question is usually "how often
    /// has this rule been excepted?" rather than "what is excepted right now?".
    pub fn waivers_for_clause(&self, clause_id: &str) -> Result<Vec<Waiver>> {
        let sql = format!(
            "SELECT {WAIVER_COLS} FROM waivers WHERE clause_id = ?1 ORDER BY created_at DESC, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![clause_id], row_to_waiver)?;
        collect(rows)
    }

    /// Waivers that can still excuse something at `now`, for one clause.
    ///
    /// Expiry is applied as a query bound rather than a stored status, so no
    /// background job is needed and no row is rewritten when a waiver lapses.
    pub fn live_waivers_for_clause(&self, clause_id: &str, now: i64) -> Result<Vec<Waiver>> {
        let sql = format!(
            "SELECT {WAIVER_COLS} FROM waivers
              WHERE clause_id = ?1 AND status = 'active' AND expires_at > ?2
              ORDER BY created_at DESC, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![clause_id, now], row_to_waiver)?;
        collect(rows)
    }

    /// Every waiver still capable of excusing something at `now`.
    pub fn live_waivers(&self, now: i64) -> Result<Vec<Waiver>> {
        let sql = format!(
            "SELECT {WAIVER_COLS} FROM waivers
              WHERE status = 'active' AND expires_at > ?1
              ORDER BY expires_at, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![now], row_to_waiver)?;
        collect(rows)
    }
}

fn row_to_waiver(row: &Row<'_>) -> rusqlite::Result<Waiver> {
    let status: String = row.get(9)?;
    Ok(Waiver {
        id: row.get(0)?,
        clause_id: row.get(1)?,
        constitution_version: row.get(2)?,
        scope: row.get(3)?,
        reason: row.get(4)?,
        approved_by: row.get(5)?,
        created_at: row.get(6)?,
        expires_at: row.get(7)?,
        remediation_task_id: row.get(8)?,
        status: WaiverStatus::from_tag(&status).unwrap_or(WaiverStatus::Active),
        revoked_by: row.get(10)?,
        revoked_at: row.get(11)?,
        revocation_reason: row.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::store;
    use crate::GoalKind;

    const NOW: i64 = 1_000;

    fn waivable_clause(store: &LodestarStore) -> String {
        let goal = store
            .define_goal(
                GoalKind::Invariant,
                "Protect the security boundary",
                "Never commit credentials.",
                None,
                NOW,
            )
            .unwrap();
        store
            .complete_clause_contract(
                &goal.id,
                "artifact:crates/**",
                "secret scan",
                Some(crate::model::Consequence::Block),
                true,
                None,
            )
            .unwrap();
        goal.id
    }

    fn request(clause_id: &str) -> WaiverRequest {
        WaiverRequest {
            clause_id: clause_id.to_string(),
            scope: "artifact:crates/lodestar-core/**".into(),
            reason: "Release blocker; remediation tracked.".into(),
            approved_by: "monk-eee".into(),
            expires_at: NOW + 3_600,
            remediation_task_id: None,
        }
    }

    #[test]
    fn a_granted_waiver_is_live_until_it_lapses() {
        let store = store();
        let clause = waivable_clause(&store);
        let granted = store
            .grant_waiver("waiver:1", &request(&clause), NOW)
            .unwrap();
        assert_eq!(granted.status, WaiverStatus::Active);

        assert_eq!(
            store.live_waivers_for_clause(&clause, NOW).unwrap().len(),
            1
        );
        // One second past expiry it stops matching, with no write in between.
        assert!(store
            .live_waivers_for_clause(&clause, NOW + 3_600)
            .unwrap()
            .is_empty());
        // ...and the record still reads active, because expiry is not a status.
        assert_eq!(
            store.waiver("waiver:1").unwrap().unwrap().status,
            WaiverStatus::Active
        );
    }

    #[test]
    fn revocation_is_attributed_immediate_and_not_a_delete() {
        let store = store();
        let clause = waivable_clause(&store);
        store
            .grant_waiver("waiver:1", &request(&clause), NOW)
            .unwrap();

        let revoked = store
            .revoke_waiver("waiver:1", "monk-eee", "Fix landed early", NOW + 10)
            .unwrap();
        assert_eq!(revoked.status, WaiverStatus::Revoked);
        assert_eq!(revoked.revoked_by.as_deref(), Some("monk-eee"));
        assert!(store
            .live_waivers_for_clause(&clause, NOW + 20)
            .unwrap()
            .is_empty());
        // The exception happened; the record of it survives for audit.
        assert_eq!(store.waivers_for_clause(&clause).unwrap().len(), 1);
    }

    #[test]
    fn revoking_twice_is_refused_rather_than_silently_repeated() {
        let store = store();
        let clause = waivable_clause(&store);
        store
            .grant_waiver("waiver:1", &request(&clause), NOW)
            .unwrap();
        store
            .revoke_waiver("waiver:1", "monk-eee", "Fixed", NOW + 10)
            .unwrap();
        let error = store
            .revoke_waiver("waiver:1", "monk-eee", "Fixed again", NOW + 20)
            .unwrap_err();
        assert!(format!("{error}").contains("already revoked"), "{error}");
    }

    #[test]
    fn a_waiver_cannot_reference_a_clause_or_remediation_task_that_does_not_exist() {
        let store = store();
        assert!(store
            .grant_waiver("waiver:1", &request("goal:missing"), NOW)
            .is_err());

        let clause = waivable_clause(&store);
        let mut bad = request(&clause);
        bad.remediation_task_id = Some("task:ghost".into());
        let error = store.grant_waiver("waiver:2", &bad, NOW).unwrap_err();
        assert!(format!("{error}").contains("remediation task"), "{error}");
    }

    #[test]
    fn a_clause_that_never_completed_its_contract_is_not_waivable() {
        // `waivable` defaults false, so an incomplete clause refuses exceptions
        // rather than accepting them by omission.
        let store = store();
        let goal = store
            .define_goal(GoalKind::Invariant, "Incomplete", "No contract.", None, NOW)
            .unwrap();
        let error = store
            .grant_waiver("waiver:1", &request(&goal.id), NOW)
            .unwrap_err();
        assert!(format!("{error}").contains("unwaivable"), "{error}");
    }
}
