//! Whether a registered session read memory before its first attributed
//! write, derived from the append-only telemetry trail.

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value;

use super::classify::{is_attributed_write, is_memory_read};
use super::types::MemoryHabit;
use crate::error::Result;

pub(super) const MEMORY_HABIT_LIMIT: usize = 32;
const MEMORY_HABIT_EVENT_LIMIT: i64 = 10_000;

#[derive(Default)]
struct MemoryHabitState {
    last_event_id: i64,
    memory_reads: i64,
    attributed_writes: i64,
    read_before_first_write: Option<bool>,
}

pub(super) fn memory_habits(conn: &Connection) -> Result<Vec<MemoryHabit>> {
    let mut states: HashMap<String, MemoryHabitState> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT id, name, outcome, detail
         FROM (
             SELECT id, name, outcome, detail
             FROM telemetry_events
             WHERE kind = 'tool_call' AND detail IS NOT NULL
             ORDER BY id DESC
             LIMIT ?1
         ) recent_attributed
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([MEMORY_HABIT_EVENT_LIMIT], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    for row in rows {
        let (id, name, outcome, detail) = row?;
        let Some(agent_id) = serde_json::from_str::<Value>(&detail)
            .ok()
            .and_then(|value| value.get("agent_id")?.as_str().map(str::to_string))
        else {
            continue;
        };
        if name == "open_session" && outcome == "ok" {
            states.insert(
                agent_id,
                MemoryHabitState {
                    last_event_id: id,
                    ..MemoryHabitState::default()
                },
            );
            continue;
        }
        let Some(state) = states.get_mut(&agent_id) else {
            continue;
        };
        state.last_event_id = id;
        if outcome != "ok" {
            continue;
        }
        if is_memory_read(&name) {
            state.memory_reads += 1;
        } else if is_attributed_write(&name) {
            if state.attributed_writes == 0 {
                state.read_before_first_write = Some(state.memory_reads > 0);
            }
            state.attributed_writes += 1;
        }
    }

    let mut habits: Vec<(i64, MemoryHabit)> = states
        .into_iter()
        .map(|(agent_id, state)| {
            (
                state.last_event_id,
                MemoryHabit {
                    agent_id,
                    memory_reads: state.memory_reads,
                    attributed_writes: state.attributed_writes,
                    read_before_first_write: state.read_before_first_write,
                },
            )
        })
        .collect();
    habits.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    habits.truncate(MEMORY_HABIT_LIMIT);
    Ok(habits.into_iter().map(|(_, habit)| habit).collect())
}
