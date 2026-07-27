//! The fleet view: who is working where, derived from declared context.
//!
//! Every value here is self-reported by a client and therefore advisory. Under
//! the ADR-0034 ceiling rule its enforcement power is `advisory`, which caps its
//! effective consequence at `review`: nothing in this module may block, and
//! nothing in it should be read as a guarantee. The mechanical controls remain
//! the publisher's ancestor check and conformance.
//!
//! The declared context itself is [`mindleak_session::SessionContext`] — the
//! same type both planes already parse from `open_session`, rather than a
//! second shape for one concept.

use std::collections::{BTreeMap, BTreeSet};

use mindleak_session::SessionContext;
use serde::{Deserialize, Serialize};

/// How far behind its declared base a session reported itself to be.
///
/// `Unknown` is a first-class answer rather than a zero: a session that declared
/// nothing is not up to date, it is unmeasured, and reporting those two as the
/// same thing is the failure ADR-0035 decision 6 exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "commits")]
pub enum Staleness {
    Unknown,
    Current,
    Behind(i64),
}

impl Staleness {
    pub fn from_declared(behind: Option<i64>) -> Self {
        match behind {
            None => Self::Unknown,
            Some(0) => Self::Current,
            Some(count) => Self::Behind(count),
        }
    }
}

/// One live session in the fleet view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetSession {
    pub agent_id: String,
    pub context: SessionContext,
    /// When the context was declared. Exposed so a reader can discount an old
    /// declaration instead of trusting it silently.
    pub declared_at: i64,
    pub staleness: Staleness,
    /// Task ids this session currently holds a live claim on.
    pub claimed_task_ids: Vec<String>,
}

/// Whether live sessions are working from the same base.
///
/// Derived purely by comparing declared bases, which needs no Git and is honest
/// about what it does not know: sessions that declared no base are counted
/// separately rather than folded in as agreement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergence {
    /// Distinct declared bases among sessions with live claims, sorted.
    pub bases: Vec<String>,
    /// Sessions holding live claims that declared no base at all.
    pub undeclared_sessions: usize,
    /// True only when two or more *declared* bases disagree. Never true on the
    /// strength of an absent declaration.
    pub diverged: bool,
}

/// One agent parked waiting on an answer from another (ADR-0046).
///
/// Derived from unanswered addressed questions, so it states only what the
/// ledger already records. `waiter` is the agent that asked and is parked;
/// `waited_on` is the agent the question was addressed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wait {
    pub task_id: String,
    pub waiter: String,
    pub waited_on: String,
    pub asked_at: i64,
}

/// A set of agents that are each, directly or transitively, waiting on one
/// another (ADR-0046).
///
/// Nobody in the set can make progress by waiting, because every one of them is
/// waiting on someone who is also waiting. It resolves only when a human — or
/// any third party — answers one of the questions, or when the ADR-0020 parking
/// grace releases the tasks a week later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitCycle {
    /// The mutually-waiting agents, sorted.
    pub agents: Vec<String>,
    /// The parked tasks whose questions form the cycle, sorted. Answering any
    /// one of them breaks it.
    pub task_ids: Vec<String>,
}

/// The read-only fleet snapshot (ADR-0035 decision 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetView {
    pub sessions: Vec<FleetSession>,
    pub divergence: Divergence,
    /// Who is currently waiting on whom (ADR-0046), oldest question first.
    pub waits: Vec<Wait>,
    /// Wait cycles: sets of agents that can only be unstuck from outside.
    pub wait_cycles: Vec<WaitCycle>,
    /// Fixed reminder that this view informs and never gates (ADR-0034).
    pub enforcement: &'static str,
}

pub(crate) const ADVISORY_NOTE: &str =
    "advisory: self-reported context, capped at review; the publisher's ancestor check remains the control";

/// The mutually-waiting agent sets in a wait graph.
///
/// A cycle is a set of agents each reachable from the other along `waiter ->
/// waited_on` edges, so no member can be unblocked by any other member. Two
/// agents addressing each other is the common case; longer rings are found by
/// the same rule rather than by a special case for length two.
///
/// Pure and total over its input: it derives from the edges alone, so it is
/// tested without a database and cannot disagree with what the view displays.
pub fn wait_cycles(waits: &[Wait]) -> Vec<WaitCycle> {
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for wait in waits {
        edges
            .entry(wait.waiter.as_str())
            .or_default()
            .insert(wait.waited_on.as_str());
        // An agent nobody is waiting on still needs a node, or a ring through it
        // would be invisible.
        edges.entry(wait.waited_on.as_str()).or_default();
    }

    let reachable: BTreeMap<&str, BTreeSet<&str>> = edges
        .keys()
        .map(|agent| (*agent, reachable_from(agent, &edges)))
        .collect();

    let mut cycles = Vec::new();
    let mut grouped: BTreeSet<&str> = BTreeSet::new();
    for agent in edges.keys() {
        if grouped.contains(agent) {
            continue;
        }
        let mutual: BTreeSet<&str> = reachable[agent]
            .iter()
            .copied()
            .filter(|other| reachable[other].contains(agent))
            .collect();
        if mutual.len() < 2 {
            continue;
        }
        grouped.extend(mutual.iter().copied());
        let mut task_ids: Vec<String> = waits
            .iter()
            .filter(|wait| {
                mutual.contains(wait.waiter.as_str()) && mutual.contains(wait.waited_on.as_str())
            })
            .map(|wait| wait.task_id.clone())
            .collect();
        task_ids.sort();
        task_ids.dedup();
        cycles.push(WaitCycle {
            agents: mutual.into_iter().map(str::to_string).collect(),
            task_ids,
        });
    }
    cycles
}

/// Every agent reachable along wait edges, including the start when a cycle
/// returns to it.
fn reachable_from<'a>(
    start: &'a str,
    edges: &BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut frontier = vec![start];
    while let Some(agent) = frontier.pop() {
        for next in edges.get(agent).into_iter().flatten() {
            if seen.insert(*next) {
                frontier.push(next);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(task_id: &str, waiter: &str, waited_on: &str) -> Wait {
        Wait {
            task_id: task_id.to_string(),
            waiter: waiter.to_string(),
            waited_on: waited_on.to_string(),
            asked_at: 0,
        }
    }

    // ADR-0046: the case the feature made reachable. Two agents addressing each
    // other both sit in needs_input looking like legitimate waits, and before
    // this the view could not tell them apart from healthy work.
    #[test]
    fn two_agents_waiting_on_each_other_are_a_cycle() {
        let cycles = wait_cycles(&[
            wait("task:1", "alice", "bob"),
            wait("task:2", "bob", "alice"),
        ]);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].agents, vec!["alice", "bob"]);
        assert_eq!(cycles[0].task_ids, vec!["task:1", "task:2"]);
    }

    // A ring longer than two is found by the same rule, not a special case.
    #[test]
    fn a_longer_ring_is_one_cycle() {
        let cycles = wait_cycles(&[
            wait("task:1", "alice", "bob"),
            wait("task:2", "bob", "carol"),
            wait("task:3", "carol", "alice"),
        ]);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].agents, vec!["alice", "bob", "carol"]);
        assert_eq!(cycles[0].task_ids, vec!["task:1", "task:2", "task:3"]);
    }

    // A chain is not a cycle. The last agent can still answer, so the wait is
    // legitimate and reporting it as a deadlock would be a false alarm — the
    // fastest way to make an advisory signal ignored.
    #[test]
    fn a_chain_is_not_a_cycle() {
        let cycles = wait_cycles(&[
            wait("task:1", "alice", "bob"),
            wait("task:2", "bob", "carol"),
        ]);
        assert!(cycles.is_empty());
    }

    // A wait on someone outside the cycle does not drag them into it: carol can
    // still answer, so she is not stuck, and naming her would send a human to
    // the wrong agent.
    #[test]
    fn an_outside_wait_is_excluded_from_the_cycle() {
        let cycles = wait_cycles(&[
            wait("task:1", "alice", "bob"),
            wait("task:2", "bob", "alice"),
            wait("task:3", "alice", "carol"),
        ]);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].agents, vec!["alice", "bob"]);
        // task:3 waits outside the cycle, so answering it would not break it.
        assert_eq!(cycles[0].task_ids, vec!["task:1", "task:2"]);
    }

    // Two independent deadlocks are two findings, each separately breakable.
    #[test]
    fn disjoint_cycles_are_reported_separately() {
        let cycles = wait_cycles(&[
            wait("task:1", "alice", "bob"),
            wait("task:2", "bob", "alice"),
            wait("task:3", "dan", "erin"),
            wait("task:4", "erin", "dan"),
        ]);
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0].agents, vec!["alice", "bob"]);
        assert_eq!(cycles[1].agents, vec!["dan", "erin"]);
    }

    #[test]
    fn no_waits_is_no_cycles() {
        assert!(wait_cycles(&[]).is_empty());
    }
}
