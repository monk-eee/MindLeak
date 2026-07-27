//! Persistence for constitutional amendments (SPEC-CONSTITUTION §9).

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

use crate::amendment::{diff_clauses, ClauseDiff, ConstitutionAmendment};
use crate::error::{LodestarError, Result};
use crate::model::{Goal, GoalStatus};
use crate::util::short_hash;

use super::LodestarStore;

impl LodestarStore {
    /// Carry every active clause into a draft version, so an amendment only has
    /// to describe what changes.
    ///
    /// The alternative — an empty draft the author repopulates — would make
    /// every amendment a re-adoption of the whole constitution, and the diff
    /// would then report every untouched rule as removed and re-added. Each copy
    /// keeps its `slug`, which is what lets the diff match the two sides.
    pub fn copy_clauses_to_version(&self, from_version: &str, to_version: &str) -> Result<usize> {
        let clauses = self.clauses_for_version(from_version)?;
        for clause in &clauses {
            let draft = Goal {
                id: format!("goal:{}@{}", clause.slug, to_version),
                status: GoalStatus::Draft,
                constitution_version: Some(to_version.to_string()),
                superseded_by: None,
                ..clause.clone()
            };
            self.insert_clause_copy(&draft)?;
        }
        Ok(clauses.len())
    }

    /// Replace the active constitution with a reviewed draft, recording why.
    ///
    /// Deliberately a different call from `activate_constitution`, which refuses
    /// to run while a version is already active. Adopting a first constitution
    /// and changing an adopted one are different acts: only the second retires
    /// rules people are currently working under, so it demands a rationale and
    /// produces a reviewable diff. The first has nothing to diff against.
    ///
    /// Validation and both status flips share one `IMMEDIATE` transaction, so no
    /// concurrent writer can activate a second version or decide a clause
    /// proposal between the checks and the write.
    pub fn amend_constitution(
        &self,
        draft_id: &str,
        amended_by: &str,
        rationale: &str,
        now: i64,
    ) -> Result<ConstitutionAmendment> {
        let amended_by = amended_by.trim();
        let rationale = rationale.trim();
        if amended_by.is_empty() || rationale.is_empty() {
            return Err(LodestarError::Invalid(
                "an amendment requires an attributed author and a rationale".to_string(),
            ));
        }

        let before = self
            .active_constitution_version()?
            .map(|version| version.id)
            .ok_or_else(|| {
                LodestarError::Invalid(
                    "no constitution is active; adopting a first one is an activation, not an amendment"
                        .to_string(),
                )
            })?;
        if before == draft_id {
            return Err(LodestarError::Invalid(
                "a version cannot amend itself".to_string(),
            ));
        }

        let before_clauses = self.clauses_for_version(&before)?;
        let after_clauses = self.clauses_for_version(draft_id)?;
        if after_clauses.is_empty() {
            return Err(LodestarError::Invalid(format!(
                "{draft_id} has no clauses; repealing the constitution entirely is not an amendment"
            )));
        }
        let diff = diff_clauses(&before_clauses, &after_clauses);
        if diff.is_empty() {
            return Err(LodestarError::Invalid(format!(
                "{draft_id} is identical to {before}; an amendment that changes nothing is churn, not policy"
            )));
        }
        let serialized_diff = serde_json::to_string(&diff)?;
        let id = format!(
            "amendment:{}",
            short_hash(&format!("{before}|{draft_id}|{now}"))
        );

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;

        let status: Option<String> = transaction
            .query_row(
                "SELECT status FROM constitution_versions WHERE id = ?1",
                params![draft_id],
                |row| row.get(0),
            )
            .optional()?;
        let status = status.ok_or_else(|| LodestarError::NotFound(draft_id.to_string()))?;
        if status != GoalStatus::Draft.as_str() {
            return Err(LodestarError::Invalid(format!(
                "{draft_id} is {status}, not a draft; an amendment promotes a reviewed draft"
            )));
        }

        let undecided: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM pack_clause_proposals
             WHERE constitution_version = ?1 AND disposition IS NULL",
            params![draft_id],
            |row| row.get(0),
        )?;
        if undecided > 0 {
            return Err(LodestarError::Invalid(format!(
                "{undecided} clause proposal(s) under {draft_id} are undecided; an amendment cannot carry an unreviewed clause"
            )));
        }

        // Retire the outgoing version and its clauses, then promote the incoming
        // ones. Superseding rather than deleting is what lets a prior conformance
        // record keep naming the version it was judged under.
        transaction.execute(
            "UPDATE constitution_versions SET status = 'superseded' WHERE id = ?1",
            params![before],
        )?;
        transaction.execute(
            "UPDATE goals SET status = 'superseded'
             WHERE constitution_version = ?1 AND status = 'active'",
            params![before],
        )?;
        transaction.execute(
            "UPDATE constitution_versions
                SET status = 'active', activated_by = ?2, activated_at = ?3
              WHERE id = ?1 AND status = 'draft'",
            params![draft_id, amended_by, now],
        )?;
        transaction.execute(
            "UPDATE goals SET status = 'active'
             WHERE constitution_version = ?1 AND status = 'draft'",
            params![draft_id],
        )?;
        transaction.execute(
            "INSERT INTO constitution_amendments
                 (id, from_version, to_version, rationale, amended_by, created_at, diff)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                before,
                draft_id,
                rationale,
                amended_by,
                now,
                serialized_diff
            ],
        )?;
        transaction.commit()?;

        Ok(ConstitutionAmendment {
            id,
            from_version: before,
            to_version: draft_id.to_string(),
            rationale: rationale.to_string(),
            amended_by: amended_by.to_string(),
            created_at: now,
            diff,
        })
    }

    /// The amendment history, newest first — how policy got to where it is.
    pub fn amendments(&self) -> Result<Vec<ConstitutionAmendment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_version, to_version, rationale, amended_by, created_at, diff
               FROM constitution_amendments ORDER BY created_at DESC, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, from_version, to_version, rationale, amended_by, created_at, raw) = row?;
            out.push(ConstitutionAmendment {
                id,
                from_version,
                to_version,
                rationale,
                amended_by,
                created_at,
                diff: serde_json::from_str::<Vec<ClauseDiff>>(&raw)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amendment::ClauseChange;
    use crate::model::Consequence;
    use crate::store::test_support::store;
    use crate::GoalKind;

    const NOW: i64 = 1_000;

    /// An active one-clause constitution, plus an empty draft to amend into.
    fn governed(store: &LodestarStore) -> (String, String) {
        let active = store
            .create_constitution_version("constitution:v1", 1, GoalStatus::Draft, Some("a"), NOW)
            .unwrap();
        let clause = store
            .define_goal(
                GoalKind::Invariant,
                "No secrets",
                "Never commit keys.",
                None,
                NOW,
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE goals SET constitution_version = ?2, status = 'draft' WHERE id = ?1",
                params![clause.id, active.id],
            )
            .unwrap();
        store
            .activate_constitution(&active.id, "monk-eee", NOW)
            .unwrap();

        let draft = store
            .create_constitution_version("constitution:v2", 2, GoalStatus::Draft, Some("a"), NOW)
            .unwrap();
        (active.id, draft.id)
    }

    fn harden(store: &LodestarStore, version: &str) {
        store
            .conn
            .execute(
                "UPDATE goals SET consequence = 'block' WHERE constitution_version = ?1",
                params![version],
            )
            .unwrap();
    }

    #[test]
    fn carrying_clauses_forward_keeps_untouched_rules_out_of_the_diff() {
        // Otherwise every amendment would report the whole constitution as
        // removed and re-added, and the one line that actually changed would be
        // impossible to find.
        let store = store();
        let (before, draft) = governed(&store);
        assert_eq!(store.copy_clauses_to_version(&before, &draft).unwrap(), 1);
        harden(&store, &draft);

        let amendment = store
            .amend_constitution(&draft, "monk-eee", "Keys leaked once; harden it.", NOW + 10)
            .unwrap();
        assert_eq!(amendment.diff.len(), 1);
        assert_eq!(amendment.diff[0].change, ClauseChange::Changed);
        assert_eq!(amendment.diff[0].fields, vec!["consequence"]);
    }

    #[test]
    fn an_amendment_retires_the_old_version_and_promotes_the_new_one() {
        let store = store();
        let (before, draft) = governed(&store);
        store.copy_clauses_to_version(&before, &draft).unwrap();
        harden(&store, &draft);

        let amendment = store
            .amend_constitution(&draft, "monk-eee", "Harden the secrets rule.", NOW + 10)
            .unwrap();
        assert_eq!(amendment.from_version, before);
        assert_eq!(amendment.to_version, draft);
        assert_eq!(
            store.constitution_version(&before).unwrap().unwrap().status,
            GoalStatus::Superseded
        );
        assert_eq!(
            store.constitution_version(&draft).unwrap().unwrap().status,
            GoalStatus::Active
        );
        // Superseded, not deleted: a prior conformance record can still name the
        // version it was judged under.
        assert!(store.clauses_for_version(&before).unwrap().is_empty());
        assert_eq!(store.clauses_for_version(&draft).unwrap().len(), 1);
        assert_eq!(store.amendments().unwrap().len(), 1);
    }

    #[test]
    fn amending_without_an_active_constitution_is_refused() {
        // Adopting a first constitution and changing an adopted one are
        // different acts; only the second retires rules people work under.
        let store = store();
        let draft = store
            .create_constitution_version("constitution:v1", 1, GoalStatus::Draft, None, NOW)
            .unwrap();
        let error = store
            .amend_constitution(&draft.id, "monk-eee", "Because", NOW)
            .unwrap_err();
        assert!(
            format!("{error}").contains("activation, not an amendment"),
            "{error}"
        );
    }

    #[test]
    fn an_amendment_requires_attribution_and_a_rationale() {
        let store = store();
        let (before, draft) = governed(&store);
        store.copy_clauses_to_version(&before, &draft).unwrap();
        harden(&store, &draft);
        assert!(store
            .amend_constitution(&draft, "", "Because", NOW)
            .is_err());
        assert!(store
            .amend_constitution(&draft, "monk-eee", "  ", NOW)
            .is_err());
    }

    #[test]
    fn an_amendment_that_changes_nothing_is_refused() {
        // A no-op version bump would retire every clause and re-issue it
        // identically, invalidating live conformance tokens for no reason.
        let store = store();
        let (before, draft) = governed(&store);
        store.copy_clauses_to_version(&before, &draft).unwrap();
        let error = store
            .amend_constitution(&draft, "monk-eee", "Tidy up", NOW)
            .unwrap_err();
        assert!(format!("{error}").contains("changes nothing"), "{error}");
    }

    #[test]
    fn an_amendment_cannot_repeal_the_constitution_entirely() {
        // An empty constitution governs nothing while still reading as governed,
        // which is worse than having none at all.
        let store = store();
        let (_, draft) = governed(&store);
        let error = store
            .amend_constitution(&draft, "monk-eee", "Clean slate", NOW)
            .unwrap_err();
        assert!(format!("{error}").contains("no clauses"), "{error}");
    }

    #[test]
    fn an_amendment_cannot_carry_an_undecided_clause_proposal() {
        let store = store();
        let (before, draft) = governed(&store);
        store.copy_clauses_to_version(&before, &draft).unwrap();
        harden(&store, &draft);

        // A real, undecided pack proposal sitting under the draft.
        let pack = crate::policy::common_core_pack();
        store.register_policy_pack(&pack, NOW).unwrap();
        store
            .propose_policy_pack(&pack.id, &pack.version, Some(&draft), NOW)
            .unwrap();

        let error = store
            .amend_constitution(&draft, "monk-eee", "Because", NOW)
            .unwrap_err();
        assert!(format!("{error}").contains("undecided"), "{error}");
    }

    #[test]
    fn the_stored_diff_survives_a_round_trip() {
        let store = store();
        let (before, draft) = governed(&store);
        store.copy_clauses_to_version(&before, &draft).unwrap();
        harden(&store, &draft);
        store
            .amend_constitution(&draft, "monk-eee", "Harden", NOW + 10)
            .unwrap();

        let history = store.amendments().unwrap();
        let after = history[0].diff[0].after.as_ref().unwrap();
        assert_eq!(after.consequence, Some(Consequence::Block));
        assert_eq!(history[0].rationale, "Harden");
    }
}
