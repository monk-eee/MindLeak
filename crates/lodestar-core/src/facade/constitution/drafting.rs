//! Drafting and activation: propose a fresh constitution for an ungoverned
//! project, read its cited facts, and activate a fully-reviewed draft.

use crate::{
    common_core_pack, discovery::discover_project_facts, discovery::ProjectFact, now_unix,
    ConstitutionProposal, ConstitutionVersion, GoalStatus, Lodestar, LodestarError, Result,
};

impl Lodestar {
    /// Draft a constitution for an ungoverned project (SPEC-CONSTITUTION §7.3):
    /// discover cited facts from the supplied repository paths, record them as
    /// the draft's provenance, and propose the Common Core against that draft.
    ///
    /// Deterministic and model-free, and it **never activates**: the returned
    /// version is a `draft`, every Common Core clause is left undecided for a
    /// human, and every discovered fact is evidence rather than a clause. An
    /// already-active constitution is refused because changing adopted policy is
    /// an amendment, not a fresh proposal.
    pub fn propose_constitution(
        &self,
        paths: &[String],
        created_by: Option<&str>,
    ) -> Result<ConstitutionProposal> {
        if let Some(active) = self.store.active_constitution_version()? {
            return Err(LodestarError::Invalid(format!(
                "{} is already active; changing adopted policy is an amendment, not a fresh proposal",
                active.id
            )));
        }
        if let Some(draft) = self.store.draft_constitution_version()? {
            return Err(LodestarError::Invalid(format!(
                "{} is already drafted and awaiting review; resolve or activate it rather than drafting over it",
                draft.id
            )));
        }

        let now = now_unix();
        let number = self.store.next_constitution_version_number()?;
        let id = format!("constitution:v{number}");
        let version = self.store.create_constitution_version(
            &id,
            number,
            GoalStatus::Draft,
            created_by,
            now,
        )?;

        let facts = discover_project_facts(paths);
        self.store.record_project_facts(&id, &facts, now)?;

        let pack = common_core_pack();
        self.register_policy_pack(&pack)?;
        let common_core = self.propose_policy_pack(&pack.id, &pack.version, Some(&id))?;

        Ok(ConstitutionProposal {
            version,
            facts,
            common_core,
        })
    }

    /// The cited facts a drafted or active constitution was grounded in.
    pub fn constitution_facts(&self, constitution_version: &str) -> Result<Vec<ProjectFact>> {
        self.store.project_facts(constitution_version)
    }

    /// Activate a reviewed draft as the governing constitution
    /// (SPEC-CONSTITUTION §7.5).
    ///
    /// One atomic transaction validates and promotes: it refuses a draft with
    /// any undecided clause proposal, a draft with no clauses at all, anything
    /// that is not a draft, and activation while another version is already
    /// active. Adopted clauses are promoted with their version, so nothing
    /// governs until this call succeeds. Activation is attributed and requires
    /// no model.
    pub fn activate_constitution(
        &self,
        draft_id: &str,
        activated_by: &str,
    ) -> Result<ConstitutionVersion> {
        let activated_by = activated_by.trim();
        if activated_by.is_empty() {
            return Err(LodestarError::Invalid(
                "activating a constitution requires an attributed authority".to_string(),
            ));
        }
        self.store
            .activate_constitution(draft_id, activated_by, now_unix())
    }
}
