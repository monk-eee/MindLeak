//! Persistence for typed controls bound to constitutional clauses (ADR-0034).

use rusqlite::{params, OptionalExtension, Row};

use crate::controls::{Control, ControlKind, ControlStatus, EnforcementPower};
use crate::error::{LodestarError, Result};

use super::{collect, LodestarStore};

const CONTROL_COLS: &str = "id, clause_id, kind, power, version, configuration, status";

impl LodestarStore {
    /// Bind one versioned control to a clause.
    ///
    /// Re-registering the same id with the same shape is idempotent. Changing
    /// what a control *does* requires an explicit version bump from the caller,
    /// so an observation can always be matched against the version its clause
    /// bound — a silent redefinition would let stale evidence resolve as though
    /// it described the current mechanism.
    pub fn register_control(&self, control: &Control, now: i64) -> Result<Control> {
        if control.clause_id.trim().is_empty() || control.id.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "a control requires an id and a clause id".to_string(),
            ));
        }
        let clause_exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM goals WHERE id = ?1",
                params![control.clause_id],
                |row| row.get(0),
            )
            .optional()?;
        if clause_exists.is_none() {
            return Err(LodestarError::NotFound(format!(
                "clause {} for control {}",
                control.clause_id, control.id
            )));
        }

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT version FROM controls WHERE id = ?1",
                params![control.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing > control.version {
                return Err(LodestarError::Invalid(format!(
                    "control {} is already registered at version {existing}; a control version never moves backwards",
                    control.id
                )));
            }
        }

        self.conn.execute(
            "INSERT INTO controls
                 (id, clause_id, kind, power, version, configuration, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                 clause_id = excluded.clause_id,
                 kind = excluded.kind,
                 power = excluded.power,
                 version = excluded.version,
                 configuration = excluded.configuration,
                 status = excluded.status",
            params![
                control.id,
                control.clause_id,
                control.kind_tag(),
                control.power.as_str(),
                control.version,
                control.configuration,
                control.status_tag(),
                now
            ],
        )?;
        self.control(&control.id)?
            .ok_or_else(|| LodestarError::NotFound(control.id.clone()))
    }

    /// One control by id.
    pub fn control(&self, id: &str) -> Result<Option<Control>> {
        let sql = format!("SELECT {CONTROL_COLS} FROM controls WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![id], row_to_control)
            .optional()?)
    }

    /// Every control bound to one clause, newest binding order stable by id.
    pub fn controls_for_clause(&self, clause_id: &str) -> Result<Vec<Control>> {
        let sql = format!("SELECT {CONTROL_COLS} FROM controls WHERE clause_id = ?1 ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![clause_id], row_to_control)?;
        collect(rows)
    }

    /// Retire a control without deleting it, so historical observations remain
    /// resolvable against the mechanism that produced them.
    pub fn retire_control(&self, id: &str) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE controls SET status = 'retired' WHERE id = ?1 AND status <> 'retired'",
            params![id],
        )?;
        Ok(changed > 0)
    }
}

impl Control {
    fn kind_tag(&self) -> &'static str {
        match self.kind {
            ControlKind::Check => "check",
            ControlKind::Threshold => "threshold",
            ControlKind::Ratchet => "ratchet",
            ControlKind::Procedure => "procedure",
            ControlKind::Judgment => "judgment",
        }
    }

    fn status_tag(&self) -> &'static str {
        match self.status {
            ControlStatus::Active => "active",
            ControlStatus::Retired => "retired",
        }
    }
}

fn control_kind_from_tag(tag: &str) -> Option<ControlKind> {
    match tag {
        "check" => Some(ControlKind::Check),
        "threshold" => Some(ControlKind::Threshold),
        "ratchet" => Some(ControlKind::Ratchet),
        "procedure" => Some(ControlKind::Procedure),
        "judgment" => Some(ControlKind::Judgment),
        _ => None,
    }
}

fn row_to_control(row: &Row) -> rusqlite::Result<Control> {
    let kind: String = row.get(2)?;
    let power: String = row.get(3)?;
    let status: String = row.get(6)?;
    Ok(Control {
        id: row.get(0)?,
        clause_id: row.get(1)?,
        kind: control_kind_from_tag(&kind).unwrap_or(ControlKind::Check),
        power: EnforcementPower::from_tag(&power).unwrap_or(EnforcementPower::Advisory),
        version: row.get(4)?,
        configuration: row.get(5)?,
        status: if status == "retired" {
            ControlStatus::Retired
        } else {
            ControlStatus::Active
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::{goal, store, NOW};

    fn control(id: &str, clause_id: &str, version: i64) -> Control {
        Control {
            id: id.into(),
            clause_id: clause_id.into(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version,
            configuration: None,
            status: ControlStatus::Active,
        }
    }

    #[test]
    fn a_control_must_bind_to_a_clause_that_exists() {
        let store = store();
        let error = store
            .register_control(&control("control:orphan", "goal:missing", 1), NOW)
            .unwrap_err()
            .to_string();
        assert!(error.contains("goal:missing"), "{error}");
    }

    #[test]
    fn registering_the_same_control_twice_is_idempotent() {
        let store = store();
        let clause = goal(&store);
        let first = store
            .register_control(&control("control:a", &clause.id, 1), NOW)
            .unwrap();
        let again = store
            .register_control(&control("control:a", &clause.id, 1), NOW + 1)
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(store.controls_for_clause(&clause.id).unwrap().len(), 1);
    }

    #[test]
    fn a_control_version_never_moves_backwards() {
        // An observation is matched against the version its clause bound, so a
        // silent downgrade would let stale evidence look current.
        let store = store();
        let clause = goal(&store);
        store
            .register_control(&control("control:a", &clause.id, 3), NOW)
            .unwrap();
        let error = store
            .register_control(&control("control:a", &clause.id, 2), NOW + 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("never moves backwards"), "{error}");
    }

    #[test]
    fn retiring_keeps_the_control_resolvable_rather_than_deleting_it() {
        let store = store();
        let clause = goal(&store);
        store
            .register_control(&control("control:a", &clause.id, 1), NOW)
            .unwrap();
        assert!(store.retire_control("control:a").unwrap());
        assert!(!store.retire_control("control:a").unwrap());

        let retired = store.control("control:a").unwrap().unwrap();
        assert_eq!(retired.status, ControlStatus::Retired);
        assert_eq!(store.controls_for_clause(&clause.id).unwrap().len(), 1);
    }
}
