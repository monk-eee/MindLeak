use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};

use crate::model::{ClauseOrigin, Goal, GoalStatus};
use crate::policy::{
    ConstitutionPack, PackClause, PackClauseDisposition, PackClauseProposal, PackClauseProvenance,
    PackConflict, PackProposalBatch, PackReviewOutcome,
};
use crate::util::{short_hash, slugify};
use crate::{LodestarError, Result};

use super::goals::insert_goal_on;
use super::{collect, LodestarStore};

const PROPOSAL_COLS: &str = "id, pack_id, pack_version, pack_digest, constitution_version, \
     clause_json, disposition, reviewed_by, review_reason, reviewed_at, adopted_goal_id, created_at";

impl LodestarStore {
    pub fn register_policy_pack(
        &self,
        pack: &ConstitutionPack,
        now: i64,
    ) -> Result<ConstitutionPack> {
        pack.validate()?;
        if !pack
            .compatible_engine_versions
            .iter()
            .any(|version| version == "*" || version == env!("CARGO_PKG_VERSION"))
        {
            return Err(LodestarError::Invalid(format!(
                "policy pack {}@{} is not compatible with Lodestar {}",
                pack.id,
                pack.version,
                env!("CARGO_PKG_VERSION")
            )));
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT digest FROM policy_packs WHERE pack_id = ?1 AND version = ?2",
                params![pack.id, pack.version],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(digest) = existing {
            if digest != pack.digest {
                return Err(LodestarError::Invalid(format!(
                    "policy pack {}@{} is immutable and already registered with a different digest",
                    pack.id, pack.version
                )));
            }
            transaction.commit()?;
            return Ok(pack.clone());
        }

        transaction.execute(
            "INSERT INTO policy_packs
                 (pack_id, version, digest, title, description, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                pack.id,
                pack.version,
                pack.digest,
                pack.title,
                pack.description,
                serde_json::to_string(pack)?,
                now
            ],
        )?;
        for conflict in &pack.conflicts {
            transaction.execute(
                "INSERT INTO policy_pack_conflicts
                     (pack_id, pack_version, conflicting_pack_id, reason)
                 VALUES (?1, ?2, ?3, ?4)",
                params![pack.id, pack.version, conflict.pack_id, conflict.reason],
            )?;
        }
        transaction.commit()?;
        Ok(pack.clone())
    }

    pub fn get_policy_pack(&self, id: &str, version: &str) -> Result<Option<ConstitutionPack>> {
        let content: Option<String> = self
            .conn
            .query_row(
                "SELECT content_json FROM policy_packs WHERE pack_id = ?1 AND version = ?2",
                params![id, version],
                |row| row.get(0),
            )
            .optional()?;
        content
            .map(|json| Ok(serde_json::from_str(&json)?))
            .transpose()
    }

    pub fn propose_policy_pack(
        &self,
        pack_id: &str,
        version: &str,
        constitution_version: Option<&str>,
        now: i64,
    ) -> Result<PackProposalBatch> {
        let pack = self
            .get_policy_pack(pack_id, version)?
            .ok_or_else(|| LodestarError::NotFound(format!("{pack_id}@{version}")))?;
        let context = constitution_version.unwrap_or("");
        if !context.is_empty() {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM constitution_versions WHERE id = ?1)",
                [context],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(LodestarError::NotFound(context.to_string()));
            }
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        for clause in &pack.clauses {
            let identity = format!("{}:{}:{}:{}", pack.id, pack.version, context, clause.key);
            let id = format!("pack-proposal:{}", short_hash(&identity));
            transaction.execute(
                "INSERT OR IGNORE INTO pack_clause_proposals
                     (id, pack_id, pack_version, pack_digest, constitution_version,
                      clause_key, clause_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    pack.id,
                    pack.version,
                    pack.digest,
                    context,
                    clause.key,
                    serde_json::to_string(clause)?,
                    now
                ],
            )?;
        }
        transaction.commit()?;

        let proposals =
            self.policy_pack_proposals(pack_id, version, constitution_version, false)?;
        let conflicts = self.unresolved_pack_conflicts(pack_id, version, context)?;
        Ok(PackProposalBatch {
            needs_human: !conflicts.is_empty(),
            proposals,
            conflicts,
        })
    }

    pub fn policy_pack_proposals(
        &self,
        pack_id: &str,
        version: &str,
        constitution_version: Option<&str>,
        include_decided: bool,
    ) -> Result<Vec<PackClauseProposal>> {
        let sql = format!(
            "SELECT {PROPOSAL_COLS} FROM pack_clause_proposals
             WHERE pack_id = ?1 AND pack_version = ?2 AND constitution_version = ?3 {}
             ORDER BY clause_json",
            if include_decided {
                ""
            } else {
                "AND disposition IS NULL"
            }
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            params![pack_id, version, constitution_version.unwrap_or("")],
            row_to_proposal,
        )?;
        collect(rows)
    }

    pub fn review_pack_clause(
        &self,
        proposal_id: &str,
        disposition: PackClauseDisposition,
        tailored_clause: Option<&PackClause>,
        actor: &str,
        reason: Option<&str>,
        now: i64,
    ) -> Result<PackReviewOutcome> {
        if actor.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "pack clause review requires an attributed actor".to_string(),
            ));
        }
        if disposition == PackClauseDisposition::Rejected
            && reason.is_none_or(|value| value.trim().is_empty())
        {
            return Err(LodestarError::Invalid(
                "rejecting a pack clause requires a reason".to_string(),
            ));
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let proposal = get_proposal_on(&transaction, proposal_id)?
            .ok_or_else(|| LodestarError::NotFound(proposal_id.to_string()))?;
        if let Some(existing) = proposal.disposition {
            if existing != disposition {
                return Err(LodestarError::Invalid(format!(
                    "pack clause proposal {proposal_id} is already {}",
                    existing.as_str()
                )));
            }
            transaction.commit()?;
            let goal = proposal
                .adopted_goal_id
                .as_deref()
                .map(|goal_id| self.get_goal(goal_id))
                .transpose()?
                .flatten();
            return Ok(PackReviewOutcome { proposal, goal });
        }

        let selected_clause = match disposition {
            PackClauseDisposition::Tailored => {
                let clause = tailored_clause.ok_or_else(|| {
                    LodestarError::Invalid(
                        "tailored disposition requires a tailored clause".to_string(),
                    )
                })?;
                validate_tailored_clause(&proposal.clause, clause)?;
                clause.clone()
            }
            PackClauseDisposition::Adopted => {
                if tailored_clause.is_some() {
                    return Err(LodestarError::Invalid(
                        "use the tailored disposition when changing a pack clause".to_string(),
                    ));
                }
                proposal.clause.clone()
            }
            PackClauseDisposition::Rejected => proposal.clause.clone(),
        };

        let mut goal = None;
        let mut adopted_goal_id = None;
        if disposition != PackClauseDisposition::Rejected {
            let conflicts = unresolved_conflicts_on(
                &transaction,
                &proposal.pack_id,
                &proposal.pack_version,
                proposal.constitution_version.as_deref().unwrap_or(""),
            )?;
            if !conflicts.is_empty() {
                return Err(LodestarError::Invalid(format!(
                    "policy pack conflict requires human resolution before adoption: {}",
                    conflicts
                        .iter()
                        .map(|conflict| conflict.pack_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            let prior_version: Option<String> = transaction
                .query_row(
                    "SELECT p.pack_version FROM pack_clause_provenance p
                     JOIN goals g ON g.id = p.goal_id
                     WHERE p.pack_id = ?1 AND p.clause_key = ?2
                       AND p.pack_version <> ?3 AND g.status = 'active'
                     LIMIT 1",
                    params![proposal.pack_id, proposal.clause.key, proposal.pack_version],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(prior_version) = prior_version {
                return Err(LodestarError::Invalid(format!(
                    "policy pack upgrade {}@{} cannot replace adopted clause {} from version {}; create an amendment proposal",
                    proposal.pack_id, proposal.pack_version, proposal.clause.key, prior_version
                )));
            }

            let status =
                constitution_status_on(&transaction, proposal.constitution_version.as_deref())?;
            let identity = format!(
                "{}:{}:{}:{}",
                proposal.pack_id,
                proposal.pack_version,
                proposal.constitution_version.as_deref().unwrap_or(""),
                proposal.clause.key
            );
            let materialized = Goal {
                id: format!("goal:pack-{}", short_hash(&identity)),
                slug: slugify(&selected_clause.title),
                kind: selected_clause.kind,
                title: selected_clause.title.clone(),
                statement: selected_clause.statement.clone(),
                status,
                version: 1,
                parent_id: None,
                superseded_by: None,
                reason: reason.map(str::to_string),
                created_at: now,
                constitution_version: proposal.constitution_version.clone(),
                rationale: Some(selected_clause.rationale.clone()),
                scope: selected_clause.default_scope.clone(),
                evidence_contract: selected_clause.evidence_contract.clone(),
                consequence: selected_clause.default_consequence,
                waivable: false,
                waiver_authority: None,
                origin: ClauseOrigin::Pack,
            };
            insert_goal_on(&transaction, &materialized)?;
            transaction.execute(
                "INSERT INTO pack_clause_provenance
                     (goal_id, pack_id, pack_version, pack_digest, clause_key, clause_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    materialized.id,
                    proposal.pack_id,
                    proposal.pack_version,
                    proposal.pack_digest,
                    proposal.clause.key,
                    serde_json::to_string(&proposal.clause)?
                ],
            )?;
            adopted_goal_id = Some(materialized.id.clone());
            goal = Some(materialized);
        }

        transaction.execute(
            "UPDATE pack_clause_proposals
             SET disposition = ?2, reviewed_by = ?3, review_reason = ?4,
                 reviewed_at = ?5, adopted_goal_id = ?6
             WHERE id = ?1 AND disposition IS NULL",
            params![
                proposal_id,
                disposition.as_str(),
                actor,
                reason,
                now,
                adopted_goal_id
            ],
        )?;
        transaction.commit()?;
        let proposal = self
            .get_pack_clause_proposal(proposal_id)?
            .ok_or_else(|| LodestarError::NotFound(proposal_id.to_string()))?;
        Ok(PackReviewOutcome { proposal, goal })
    }

    pub fn get_pack_clause_proposal(&self, id: &str) -> Result<Option<PackClauseProposal>> {
        get_proposal_on(&self.conn, id)
    }

    pub fn pack_clause_provenance(&self, goal_id: &str) -> Result<Option<PackClauseProvenance>> {
        self.conn
            .query_row(
                "SELECT goal_id, pack_id, pack_version, pack_digest, clause_key, clause_json
                 FROM pack_clause_provenance WHERE goal_id = ?1",
                [goal_id],
                |row| {
                    let json: String = row.get(5)?;
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        json,
                    ))
                },
            )
            .optional()?
            .map(
                |(goal_id, pack_id, pack_version, pack_digest, clause_key, json)| {
                    Ok(PackClauseProvenance {
                        goal_id,
                        pack_id,
                        pack_version,
                        pack_digest,
                        clause_key,
                        source_clause: serde_json::from_str(&json)?,
                    })
                },
            )
            .transpose()
    }

    fn unresolved_pack_conflicts(
        &self,
        pack_id: &str,
        version: &str,
        context: &str,
    ) -> Result<Vec<PackConflict>> {
        unresolved_conflicts_on(&self.conn, pack_id, version, context)
    }
}

fn get_proposal_on(connection: &Connection, id: &str) -> Result<Option<PackClauseProposal>> {
    let sql = format!("SELECT {PROPOSAL_COLS} FROM pack_clause_proposals WHERE id = ?1");
    Ok(connection
        .query_row(&sql, [id], row_to_proposal)
        .optional()?)
}

fn row_to_proposal(row: &Row) -> rusqlite::Result<PackClauseProposal> {
    let clause_json: String = row.get(5)?;
    let disposition: Option<String> = row.get(6)?;
    Ok(PackClauseProposal {
        id: row.get(0)?,
        pack_id: row.get(1)?,
        pack_version: row.get(2)?,
        pack_digest: row.get(3)?,
        constitution_version: match row.get::<_, String>(4)? {
            value if value.is_empty() => None,
            value => Some(value),
        },
        clause: serde_json::from_str(&clause_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        disposition: disposition
            .as_deref()
            .and_then(PackClauseDisposition::from_tag),
        reviewed_by: row.get(7)?,
        review_reason: row.get(8)?,
        reviewed_at: row.get(9)?,
        adopted_goal_id: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn unresolved_conflicts_on(
    connection: &Connection,
    pack_id: &str,
    version: &str,
    context: &str,
) -> Result<Vec<PackConflict>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT other_pack, reason FROM (
             SELECT c.conflicting_pack_id AS other_pack, c.reason AS reason
             FROM policy_pack_conflicts c
             WHERE c.pack_id = ?1 AND c.pack_version = ?2
             UNION ALL
             SELECT c.pack_id AS other_pack, c.reason AS reason
             FROM policy_pack_conflicts c
             WHERE c.conflicting_pack_id = ?1
         ) conflicts
         WHERE EXISTS (
             SELECT 1 FROM pack_clause_proposals p
             WHERE p.pack_id = conflicts.other_pack
               AND p.constitution_version = ?3
               AND (p.disposition IS NULL OR p.disposition IN ('adopted', 'tailored'))
         )
         ORDER BY other_pack",
    )?;
    let rows = statement.query_map(params![pack_id, version, context], |row| {
        Ok(PackConflict {
            pack_id: row.get(0)?,
            reason: row.get(1)?,
        })
    })?;
    collect(rows)
}

fn constitution_status_on(
    connection: &Connection,
    constitution_version: Option<&str>,
) -> Result<GoalStatus> {
    let Some(version) = constitution_version.filter(|value| !value.is_empty()) else {
        return Ok(GoalStatus::Draft);
    };
    let status: String = connection.query_row(
        "SELECT status FROM constitution_versions WHERE id = ?1",
        [version],
        |row| row.get(0),
    )?;
    GoalStatus::from_tag(&status).ok_or_else(|| {
        LodestarError::Invalid(format!(
            "constitution {version} has invalid status {status}"
        ))
    })
}

fn validate_tailored_clause(source: &PackClause, tailored: &PackClause) -> Result<()> {
    if tailored.key != source.key
        || tailored.title.trim().is_empty()
        || tailored.statement.trim().is_empty()
        || tailored.rationale.trim().is_empty()
    {
        return Err(LodestarError::Invalid(
            "a tailored clause must preserve the source key and provide title, statement, and rationale"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Consequence;
    use crate::policy::common_core_pack;
    use crate::store::test_support::{store, NOW};

    fn active_constitution(store: &LodestarStore) {
        store
            .conn
            .execute(
                "INSERT INTO constitution_versions
                     (id, version, status, created_by, created_at, activated_by, activated_at)
                 VALUES ('constitution:v1', 1, 'active', 'test', ?1, 'test', ?1)",
                [NOW],
            )
            .unwrap();
    }

    fn one_clause_pack(id: &str, version: &str, statement: &str) -> ConstitutionPack {
        let mut pack = common_core_pack();
        pack.id = id.to_string();
        pack.version = version.to_string();
        pack.title = format!("{id} {version}");
        pack.clauses.truncate(1);
        pack.clauses[0].statement = statement.to_string();
        pack.clauses[0].default_scope = Some("repository".to_string());
        pack.clauses[0].evidence_contract = Some("review evidence".to_string());
        pack.clauses[0].default_consequence = Some(Consequence::Review);
        pack.digest = pack.computed_digest().unwrap();
        pack
    }

    #[test]
    fn adoption_materializes_a_self_contained_clause_with_source_provenance() {
        let store = store();
        active_constitution(&store);
        let pack = one_clause_pack("delivery", "1", "Require current evidence.");
        store.register_policy_pack(&pack, NOW).unwrap();
        let batch = store
            .propose_policy_pack(&pack.id, &pack.version, Some("constitution:v1"), NOW)
            .unwrap();
        let outcome = store
            .review_pack_clause(
                &batch.proposals[0].id,
                PackClauseDisposition::Adopted,
                None,
                "reviewer",
                Some("fits this project"),
                NOW + 1,
            )
            .unwrap();

        let goal = outcome.goal.unwrap();
        assert_eq!(goal.origin, ClauseOrigin::Pack);
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.statement, "Require current evidence.");
        let provenance = store.pack_clause_provenance(&goal.id).unwrap().unwrap();
        assert_eq!(provenance.pack_id, "delivery");
        assert_eq!(provenance.pack_version, "1");
        assert_eq!(provenance.pack_digest, pack.digest);
        assert_eq!(provenance.source_clause, pack.clauses[0]);
    }

    #[test]
    fn one_pack_version_is_idempotent_only_for_the_same_digest() {
        let store = store();
        let pack = one_clause_pack("delivery", "1", "Original content.");
        store.register_policy_pack(&pack, NOW).unwrap();
        store.register_policy_pack(&pack, NOW + 1).unwrap();

        let changed = one_clause_pack("delivery", "1", "Different bytes.");
        assert!(store
            .register_policy_pack(&changed, NOW + 2)
            .unwrap_err()
            .to_string()
            .contains("immutable"));
        assert_eq!(
            store
                .get_policy_pack("delivery", "1")
                .unwrap()
                .unwrap()
                .digest,
            pack.digest
        );
    }

    #[test]
    fn rejection_is_durable_and_not_reproposed() {
        let store = store();
        active_constitution(&store);
        let pack = one_clause_pack("delivery", "1", "Require current evidence.");
        store.register_policy_pack(&pack, NOW).unwrap();
        let first = store
            .propose_policy_pack(&pack.id, &pack.version, Some("constitution:v1"), NOW)
            .unwrap();
        store
            .review_pack_clause(
                &first.proposals[0].id,
                PackClauseDisposition::Rejected,
                None,
                "reviewer",
                Some("not appropriate here"),
                NOW + 1,
            )
            .unwrap();

        let retry = store
            .propose_policy_pack(&pack.id, &pack.version, Some("constitution:v1"), NOW + 2)
            .unwrap();
        assert!(retry.proposals.is_empty());
        let history = store
            .policy_pack_proposals(&pack.id, &pack.version, Some("constitution:v1"), true)
            .unwrap();
        assert_eq!(
            history[0].disposition,
            Some(PackClauseDisposition::Rejected)
        );
        assert_eq!(history[0].reviewed_by.as_deref(), Some("reviewer"));
    }

    #[test]
    fn conflicting_packs_require_an_explicit_rejection_before_adoption() {
        let store = store();
        active_constitution(&store);
        let mut first = one_clause_pack("strict", "1", "Require strict review.");
        first.conflicts.push(PackConflict {
            pack_id: "permissive".to_string(),
            reason: "opposed review models".to_string(),
        });
        first.digest = first.computed_digest().unwrap();
        let second = one_clause_pack("permissive", "1", "Permit automatic review.");
        store.register_policy_pack(&first, NOW).unwrap();
        store.register_policy_pack(&second, NOW).unwrap();
        let first_batch = store
            .propose_policy_pack("strict", "1", Some("constitution:v1"), NOW)
            .unwrap();
        let second_batch = store
            .propose_policy_pack("permissive", "1", Some("constitution:v1"), NOW)
            .unwrap();
        assert!(second_batch.needs_human);
        assert!(store
            .review_pack_clause(
                &first_batch.proposals[0].id,
                PackClauseDisposition::Adopted,
                None,
                "reviewer",
                None,
                NOW + 1,
            )
            .unwrap_err()
            .to_string()
            .contains("conflict"));

        store
            .review_pack_clause(
                &second_batch.proposals[0].id,
                PackClauseDisposition::Rejected,
                None,
                "reviewer",
                Some("choose the strict pack"),
                NOW + 2,
            )
            .unwrap();
        assert!(store
            .review_pack_clause(
                &first_batch.proposals[0].id,
                PackClauseDisposition::Adopted,
                None,
                "reviewer",
                None,
                NOW + 3,
            )
            .unwrap()
            .goal
            .is_some());
    }

    #[test]
    fn upstream_version_cannot_change_an_adopted_local_clause() {
        let store = store();
        active_constitution(&store);
        let first = one_clause_pack("delivery", "1", "Original local law.");
        store.register_policy_pack(&first, NOW).unwrap();
        let first_batch = store
            .propose_policy_pack("delivery", "1", Some("constitution:v1"), NOW)
            .unwrap();
        let adopted = store
            .review_pack_clause(
                &first_batch.proposals[0].id,
                PackClauseDisposition::Adopted,
                None,
                "reviewer",
                None,
                NOW + 1,
            )
            .unwrap()
            .goal
            .unwrap();

        let second = one_clause_pack("delivery", "2", "Changed upstream text.");
        store.register_policy_pack(&second, NOW + 2).unwrap();
        let second_batch = store
            .propose_policy_pack("delivery", "2", Some("constitution:v1"), NOW + 2)
            .unwrap();
        assert!(store
            .review_pack_clause(
                &second_batch.proposals[0].id,
                PackClauseDisposition::Adopted,
                None,
                "reviewer",
                None,
                NOW + 3,
            )
            .unwrap_err()
            .to_string()
            .contains("amendment"));
        assert_eq!(
            store.get_goal(&adopted.id).unwrap().unwrap().statement,
            "Original local law."
        );
    }
}
