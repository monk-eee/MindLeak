//! Reviewed ratchets: a baseline a maintainer accepted, and the direction
//! a measure may move from it.
//!
//! Split out of `controls.rs` (see `super`); the code is unchanged.

use super::*;

/// Which way a measured value may not move relative to its baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RatchetDirection {
    /// The measurement must not fall below the baseline — coverage, pass rate.
    NonDecreasing,
    /// The measurement must not rise above the baseline — warning count, p95.
    NonIncreasing,
}

/// The value a ratchet compares against, and who accepted it.
///
/// A baseline is attributed on purpose. SPEC-CONSTITUTION §4 lists "whether the
/// baseline was trustworthy" among the questions a ratchet cannot answer about
/// itself, so the answer has to arrive from outside the mechanism.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RatchetBaseline {
    pub value: f64,
    pub reviewed_by: String,
    pub reviewed_at: i64,
}

/// A reviewed ratchet: one metric, one direction, one attributed baseline.
///
/// Deliberately generic. The engine ships no coverage ratchet, because §4 says a
/// ratchet cannot determine whether coverage is the right proxy for confidence —
/// baking that judgement into the engine would answer, on every project's
/// behalf, the one question the mechanism is not entitled to answer. A project
/// registers the ratchets its own clauses justify. See ADR-0037.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ratchet {
    pub control_id: String,
    pub clause_id: String,
    /// What is measured, for the audit trail: `coverage.line_pct`, `warnings`.
    pub metric: String,
    pub direction: RatchetDirection,
    /// Movement no larger than this is noise, not a regression.
    pub tolerance: f64,
    /// `None` until someone accepts one. A ratchet without a baseline reports
    /// `Unknown` — never `Pass`.
    pub baseline: Option<RatchetBaseline>,
    pub version: i64,
}

#[derive(Serialize, Deserialize)]
struct RatchetConfig {
    metric: String,
    direction: RatchetDirection,
    tolerance: f64,
    baseline: Option<RatchetBaseline>,
}

fn finite(value: f64, what: &str) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(LodestarError::Invalid(format!(
            "{what} must be a finite number"
        )))
    }
}

impl Ratchet {
    /// A ratchet with no baseline yet. It reports `Unknown` until one is
    /// accepted, which is the honest state for a mechanism that has nothing to
    /// compare against.
    pub fn new(
        control_id: &str,
        clause_id: &str,
        metric: &str,
        direction: RatchetDirection,
        tolerance: f64,
    ) -> Result<Ratchet> {
        if control_id.trim().is_empty() || clause_id.trim().is_empty() || metric.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "a ratchet requires a control id, a clause id, and a metric".to_string(),
            ));
        }
        let tolerance = finite(tolerance, "ratchet tolerance")?;
        if tolerance < 0.0 {
            return Err(LodestarError::Invalid(
                "ratchet tolerance must not be negative".to_string(),
            ));
        }
        Ok(Ratchet {
            control_id: control_id.to_string(),
            clause_id: clause_id.to_string(),
            metric: metric.to_string(),
            direction,
            tolerance,
            baseline: None,
            version: 1,
        })
    }

    /// The registrable control this ratchet is.
    ///
    /// Power is `observed`, never `mechanical`: a ratchet reads a report someone
    /// else produced and proves what already happened. It stopped nothing. Under
    /// the ADR-0034 ceiling that caps it at `review`, which is the point —
    /// whether a particular regression is acceptable is a judgement about the
    /// change, and §4 lists exactly that among the things a ratchet cannot
    /// decide.
    pub fn control(&self) -> Result<Control> {
        Ok(Control {
            id: self.control_id.clone(),
            clause_id: self.clause_id.clone(),
            kind: ControlKind::Ratchet,
            power: EnforcementPower::Observed,
            version: self.version,
            configuration: Some(serde_json::to_string(&RatchetConfig {
                metric: self.metric.clone(),
                direction: self.direction,
                tolerance: self.tolerance,
                baseline: self.baseline.clone(),
            })?),
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
    }

    /// Read a ratchet back out of the control it was registered as.
    pub fn from_control(control: &Control) -> Result<Ratchet> {
        if control.kind != ControlKind::Ratchet {
            return Err(LodestarError::Invalid(format!(
                "control {} is a {:?} control, not a ratchet",
                control.id, control.kind
            )));
        }
        let raw = control.configuration.as_deref().ok_or_else(|| {
            LodestarError::Invalid(format!(
                "ratchet {} has no configuration; it cannot say what it measures",
                control.id
            ))
        })?;
        let config: RatchetConfig = serde_json::from_str(raw)?;
        Ok(Ratchet {
            control_id: control.id.clone(),
            clause_id: control.clause_id.clone(),
            metric: config.metric,
            direction: config.direction,
            tolerance: config.tolerance,
            baseline: config.baseline,
            version: control.version,
        })
    }

    /// Accept a new baseline. Attributed, and it bumps the control version.
    ///
    /// A ratchet never does this to itself. A mechanism that adopts whatever it
    /// last measured launders a regression into the new normal — one bad run and
    /// the ratchet has quietly ratcheted *down*, still reporting green. The
    /// version bump is the second half: an observation taken against the old
    /// baseline resolves as `unknown` rather than being re-judged against a
    /// number it never saw.
    pub fn with_reviewed_baseline(
        &self,
        value: f64,
        reviewed_by: &str,
        reviewed_at: i64,
    ) -> Result<Ratchet> {
        if reviewed_by.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "a ratchet baseline requires an attributed reviewer".to_string(),
            ));
        }
        let value = finite(value, "ratchet baseline")?;
        Ok(Ratchet {
            baseline: Some(RatchetBaseline {
                value,
                reviewed_by: reviewed_by.to_string(),
                reviewed_at,
            }),
            version: self.version + 1,
            ..self.clone()
        })
    }

    /// Compare one measurement against the reviewed baseline.
    ///
    /// The result is a report, not a verdict: conformance maps it through the
    /// clause that authorises the ratchet (§4).
    pub fn observe(
        &self,
        measured: f64,
        scope: &str,
        evidence_refs: Vec<String>,
        evaluated_at: i64,
    ) -> Result<ControlObservation> {
        let measured = finite(measured, "ratchet measurement")?;
        let status = match &self.baseline {
            // No reviewed baseline means nothing to compare against. Reporting
            // `Pass` here would let an unbaselined ratchet certify conformance
            // it never checked.
            None => ObservationStatus::Unknown,
            Some(baseline) => {
                let regression = match self.direction {
                    RatchetDirection::NonDecreasing => baseline.value - measured,
                    RatchetDirection::NonIncreasing => measured - baseline.value,
                };
                if regression > self.tolerance {
                    ObservationStatus::Fail
                } else {
                    ObservationStatus::Pass
                }
            }
        };
        Ok(ControlObservation {
            control_id: self.control_id.clone(),
            clause_id: self.clause_id.clone(),
            control_version: self.version,
            scope: scope.to_string(),
            status,
            measurements: Some(serde_json::to_string(&serde_json::json!({
                "metric": self.metric,
                "measured": measured,
                "direction": self.direction,
                "tolerance": self.tolerance,
            }))?),
            baseline: self
                .baseline
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            evidence_refs,
            evaluated_at,
        })
    }
}

/// Resolve when the declared consequence is supplied directly rather than read
/// from the clause. `None` means no active clause authorises the control.
///
/// A `forbid_change` lock uses this entry point because its binding mode *is*
/// the declaration (ADR-0036): a human who placed a lock already chose the
/// consequence, so reading it from the clause's `consequence` field would let an
/// incomplete enforcement contract silently soften a deliberate act.
pub fn resolve_with_declared(
    declared: Option<Consequence>,
    control: &Control,
    observation: &ControlObservation,
) -> ControlResolution {
    let resolution =
        |status: ObservationStatus, effective: Consequence, finding: String| ControlResolution {
            control_id: control.id.clone(),
            clause_id: control.clause_id.clone(),
            status,
            effective,
            finding,
        };

    let Some(declared) = declared else {
        return resolution(
            observation.status,
            Consequence::Advise,
            format!(
                "control {} serves no active clause; an orphan control reports but cannot escalate",
                control.id
            ),
        );
    };

    if control.status == ControlStatus::Retired {
        return resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} is retired; its report is not current",
                control.id
            ),
        );
    }

    if observation.control_version != control.version {
        return resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} reported at version {} but is bound at version {}; treating as unknown",
                control.id, observation.control_version, control.version
            ),
        );
    }

    match observation.status {
        ObservationStatus::Pass => resolution(
            ObservationStatus::Pass,
            Consequence::Advise,
            format!(
                "control {} satisfied clause {}",
                control.id, control.clause_id
            ),
        ),
        ObservationStatus::Unknown => resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} could not determine an answer; absence of evidence is not conformance",
                control.id
            ),
        ),
        ObservationStatus::Fail => {
            let ceiling = control.power.ceiling();
            let effective = min_consequence(declared, ceiling);
            let finding = if effective < declared {
                format!(
                    "control {} failed clause {}; clause declares {} but a {} mechanism caps this at {}",
                    control.id,
                    control.clause_id,
                    declared.as_str(),
                    control.power.as_str(),
                    effective.as_str()
                )
            } else {
                format!(
                    "control {} failed clause {}; resolves {}",
                    control.id,
                    control.clause_id,
                    effective.as_str()
                )
            };
            resolution(ObservationStatus::Fail, effective, finding)
        }
    }
}
