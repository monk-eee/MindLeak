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

/// The read-only fleet snapshot (ADR-0035 decision 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetView {
    pub sessions: Vec<FleetSession>,
    pub divergence: Divergence,
    /// Fixed reminder that this view informs and never gates (ADR-0034).
    pub enforcement: &'static str,
}

pub(crate) const ADVISORY_NOTE: &str =
    "advisory: self-reported context, capped at review; the publisher's ancestor check remains the control";
