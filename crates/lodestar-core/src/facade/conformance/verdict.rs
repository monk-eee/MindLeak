//! Turning bounded evidence into a verdict.
//!
//! Split out of `facade/conformance.rs` (see `super`); the code is unchanged.

use super::*;

impl Lodestar {
    /// Score evidence against governing intent, then layer ADR-0022 advisory
    /// knowledge on top. The base pass owns all hard verdicts (Drift / Violation /
    /// NeedsHuman / Aligned from goals and the Constitution); the knowledge pass
    /// may only add advisory findings and, at most, nudge an otherwise-`Aligned`
    /// verdict to `NeedsHuman` — it can never harden a verdict or emit `Violation`.
    pub(super) fn evaluate_conformance(
        &self,
        evidence: &ConformanceEvidence,
        task: Option<&Task>,
    ) -> Result<ConformanceResult> {
        let mut result = self.evaluate_base_conformance(evidence, task)?;
        self.apply_knowledge_advisory(evidence, &mut result)?;
        Ok(result)
    }

    /// Consult learned knowledge (ADR-0022). When the evidence's changed nodes
    /// intersect the nodes a proven regularity references, attach an ADVISORY
    /// finding and, at most, escalate an `Aligned` verdict to `NeedsHuman` so a
    /// human looks. Knowledge is revalidated and decaying, so it MUST NOT emit a
    /// `Violation` or otherwise harden the verdict — only the Constitution
    /// (constraint / invariant goals) hard-fails. This read path stays
    /// deterministic: no LLM.
    pub(super) fn apply_knowledge_advisory(
        &self,
        evidence: &ConformanceEvidence,
        result: &mut ConformanceResult,
    ) -> Result<()> {
        if evidence.changed_node_ids.is_empty() {
            return Ok(());
        }
        let changed: HashSet<&str> = evidence
            .changed_node_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut matched = false;
        for knowledge in self.store.active_knowledge(now_unix())? {
            if knowledge
                .referenced_nodes()
                .iter()
                .any(|node| changed.contains(node.as_str()))
            {
                matched = true;
                result.findings.push(format!(
                    "advisory: learned knowledge {} — {}",
                    knowledge.id, knowledge.statement
                ));
            }
        }
        if matched && result.verdict == Verdict::Aligned {
            result.verdict = Verdict::NeedsHuman;
        }
        Ok(())
    }

    pub(super) fn evaluate_base_conformance(
        &self,
        evidence: &ConformanceEvidence,
        task: Option<&Task>,
    ) -> Result<ConformanceResult> {
        let mut findings = Vec::new();
        if evidence.changed_node_ids.is_empty() || evidence.provenance.is_empty() {
            findings.push("evidence contains no provenance-bearing mutation".to_string());
            return Ok(ConformanceResult {
                verdict: Verdict::NeedsHuman,
                findings,
            });
        }

        let task_goal_id = task.map(|t| t.goal_id.as_str());
        let covered = match task {
            Some(task) => self.store.goal_coverage(&task.id)?,
            None => Vec::new(),
        };
        let governing = self.resolve_governing_clauses_covering(
            &evidence.changed_node_ids,
            task_goal_id,
            &covered,
        )?;

        // A hard forbid_change lock overrides everything: any change is a breach.
        // It resolves through the typed-control machinery (ADR-0034) but supplies
        // its own declared consequence rather than reading the clause's
        // (ADR-0036) — a human who placed a lock already chose that power, and an
        // incomplete enforcement contract must not silently soften it.
        //
        // A valid waiver is the one thing that stands it down (§9). Note it does
        // not make the change invisible: the finding names the waiver and its
        // approver, so the audit records that an exception was used rather than
        // that nothing happened.
        if let Some((node, goal)) = governing.forbid.first() {
            let now = now_unix();
            match self.excusing_waiver(goal, node, now)? {
                Some(waiver) => findings.push(format!(
                    "{} forbids changes to {node}, waived by {} until {} ({})",
                    goal.id, waiver.approved_by, waiver.expires_at, waiver.id
                )),
                None => {
                    let control = forbid_change_control(&goal.id);
                    let observation = forbid_change_observation(&goal.id, node, now);
                    let resolved =
                        resolve_with_declared(Some(Consequence::Block), &control, &observation);
                    findings.push(format!("{} forbids changes to {node}", goal.id));
                    findings.push(resolved.finding);
                    return Ok(ConformanceResult {
                        verdict: if resolved.effective == Consequence::Block {
                            Verdict::Violation
                        } else {
                            Verdict::NeedsHuman
                        },
                        findings,
                    });
                }
            }
        }

        // Governed code touched by a goal that no covering task serves is drift.
        if !governing.other.is_empty() {
            let now = now_unix();
            let mut unwaived: Vec<String> = Vec::new();
            for (node, goal) in &governing.other {
                match self.excusing_waiver(goal, node, now)? {
                    Some(waiver) => findings.push(format!(
                        "{} governs {node} without a covering task, waived by {} until {} ({})",
                        goal.id, waiver.approved_by, waiver.expires_at, waiver.id
                    )),
                    None => unwaived.push(goal.id.clone()),
                }
            }
            unwaived.sort();
            unwaived.dedup();
            if !unwaived.is_empty() {
                findings.push(format!(
                    "governed code changed without a covering task: {}",
                    unwaived.join(", ")
                ));
                return Ok(ConformanceResult {
                    verdict: Verdict::Drift,
                    findings,
                });
            }
        }
        let touched_task_goal = !governing.in_scope.is_empty();
        let Some(task) = task else {
            findings.push("no governed code touched".to_string());
            return Ok(ConformanceResult {
                verdict: Verdict::Aligned,
                findings,
            });
        };

        // Not touching goal-bound code is a fact to record, not a verdict to
        // fail on (ADR-0060). The identical evidence with no task attached
        // aligns in the branch directly above, so failing here made the
        // *presence of a task* the thing that worsened the verdict — and made
        // work whose product is an ADR, a benchmark, a changelog fragment or
        // documentation impossible to complete, because `link_goal_to_code`
        // binds code and nothing else. `needs_human` must mean "a human needs
        // to look at this", not "the work product was not Rust".
        //
        // The finding stays: a task bound to a code goal that produced no code
        // may well be mis-scoped, and that is worth recording. It is a smell,
        // so it is recorded rather than blocking. Only a *positive* signal of a
        // problem — drift, a forbid_change lock, missing provenance, governed
        // code changed without a covering task — may downgrade a verdict.
        if !touched_task_goal {
            findings.push("evidence does not touch code bound to the task goal".to_string());
        }

        let goal = self
            .store
            .get_goal(&task.goal_id)?
            .ok_or_else(|| LodestarError::NotFound(task.goal_id.clone()))?;
        if goal.kind.is_normative() {
            match self.judge_conformance(&goal.statement, &evidence.summary) {
                Ok((verdict, rationale)) if verdict == "aligned" => {
                    findings.push(format!("semantic check aligned: {rationale}"));
                }
                Ok((verdict, rationale)) if verdict == "violation" => {
                    findings.push(format!("semantic check found a violation: {rationale}"));
                    return Ok(ConformanceResult {
                        verdict: Verdict::Violation,
                        findings,
                    });
                }
                Ok((_, rationale)) => {
                    findings.push(format!("semantic check needs human review: {rationale}"));
                    return Ok(ConformanceResult {
                        verdict: Verdict::NeedsHuman,
                        findings,
                    });
                }
                Err(_) => {
                    findings.push("semantic check unavailable".to_string());
                    return Ok(ConformanceResult {
                        verdict: Verdict::NeedsHuman,
                        findings,
                    });
                }
            }
        }

        if touched_task_goal {
            findings.push(format!("evidence covers task goal {}", task.goal_id));
        }

        // A discontinuous evidence window cannot certify itself (ADR-0048,
        // ADR-0034 ceiling rule). The window now survives a lapse so earlier
        // work stays provable, but a lapse means there was a stretch in which
        // this agent held no lease, and nothing here can tell whether work fell
        // into it. That is an unknown, not a pass.
        //
        // The cap is on the task, not on the submitted interval, and that is
        // deliberate: if it depended on whether the hole fell inside the
        // evidence span, an agent could dodge it by submitting a narrower span
        // — which is exactly the laundering this rule exists to stop.
        if task.claim_lapses > 0 {
            findings.push(format!(
                "evidence window is discontinuous: the lease lapsed {} time(s), \
                 leaving {}s unleased, which a human confirms",
                task.claim_lapses, task.unleased_seconds
            ));
            return Ok(ConformanceResult {
                verdict: Verdict::NeedsHuman,
                findings,
            });
        }

        // Declared cross-goal coverage informs; it never self-certifies
        // (ADR-0041, ADR-0034 ceiling rule). If the evidence is in scope only
        // because the task declared another goal at creation, a human confirms
        // the breadth — but the audit names which declarations it leaned on, so
        // this reads as reviewable breadth rather than as drift.
        if !governing.relied_on_coverage.is_empty() {
            findings.push(format!(
                "in scope via declared coverage, which a human confirms: {}",
                governing.relied_on_coverage.join(", ")
            ));
            return Ok(ConformanceResult {
                verdict: Verdict::NeedsHuman,
                findings,
            });
        }

        Ok(ConformanceResult {
            verdict: Verdict::Aligned,
            findings,
        })
    }
}
