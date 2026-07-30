//! Turning bounded evidence into a verdict.
//!
//! Split out of `facade/conformance.rs` (see `super`); the code is unchanged.

use super::*;

impl Lodestar {
    /// Score evidence against governing intent, then layer ADR-0022 advisory
    /// knowledge on top. The base pass owns every verdict; the knowledge pass may
    /// only add advisory findings. It cannot harden a verdict, emit `Violation`,
    /// or downgrade an otherwise-`Aligned` one (ADR-0072).
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
    /// finding so the agent sees the lesson at the moment it is relevant. The
    /// verdict is left alone: knowledge is revalidated and decaying, and topical
    /// overlap is relevance rather than a problem signal (ADR-0072, amending
    /// ADR-0022 §4). Only the Constitution (constraint / invariant goals) and the
    /// base pass decide verdicts. This read path stays deterministic: no LLM.
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
        for knowledge in self.store.active_knowledge(now_unix())? {
            if knowledge
                .referenced_nodes()
                .iter()
                .any(|node| changed.contains(node.as_str()))
            {
                result.findings.push(format!(
                    "advisory: learned knowledge {} — {}",
                    knowledge.id, knowledge.statement
                ));
            }
        }
        // The advisory informs; it does not cap (ADR-0072, amending ADR-0022 §4).
        //
        // This used to move an otherwise-aligned verdict to `needs_human`
        // whenever any active knowledge merely *referenced* a changed node.
        // Knowledge only accumulates, so the referenced set only grows, and the
        // nudge became unconditional: measured on 2026-07-30, 28 of 28
        // completions affirmed on 07-23, 3 of 34 on 07-28, and 1 of 13 that day
        // — the one survivor earning it only because nothing governed the file
        // it touched. A cap that fires on almost every task carries no
        // information, and it is not free: `blocked_by` successors open only on
        // an aligned completion, so a permanent cap freezes dependent work.
        //
        // ADR-0060 item 2 already held the principle: only a positive signal of
        // a *problem* may downgrade. Topical overlap between a changed node and
        // a recorded lesson is relevance, not a problem signal — exactly the
        // case for showing the agent the lesson, and exactly not the case for
        // doubting the work. The findings above still do that, which is the
        // whole value ADR-0022 was reaching for.
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
        // documentation impossible to complete, because `link_goal_to_artifact`
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
        //
        // The window is derived from the task log (ADR-0064 d5/d6) rather than
        // read off a counter column. It is asked for explicitly here because
        // there is no field to forget: a missing continuity check would fail
        // open, and failing open means handing out a clean receipt.
        let window = self.store.claim_window(&task.id)?;
        if !window.is_continuous() {
            findings.push(format!(
                "evidence window is discontinuous: the lease lapsed {} time(s), \
                 leaving {}s unleased, which a human confirms",
                window.lapses, window.unleased_seconds
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
