//! Policy-pack registration and proposal: register/get one immutable pack
//! version, materialize review proposals against it, and the two built-in
//! packs (Common Core, fleet-delivery).

use crate::{
    common_core_pack, fleet_delivery_pack, now_unix, ConstitutionPack, Lodestar, PackProposalBatch,
    Result,
};

impl Lodestar {
    /// Register one immutable policy-pack version. Re-registering the same
    /// digest is idempotent; the same id/version with different content is an
    /// error rather than a silent upstream rewrite.
    pub fn register_policy_pack(&self, pack: &ConstitutionPack) -> Result<ConstitutionPack> {
        self.store.register_policy_pack(pack, now_unix())
    }

    pub fn get_policy_pack(
        &self,
        pack_id: &str,
        version: &str,
    ) -> Result<Option<ConstitutionPack>> {
        self.store.get_policy_pack(pack_id, version)
    }

    /// Materialize durable review proposals for one pack. When no explicit
    /// draft/version is supplied, proposals target the current active
    /// constitution; an absent constitution leaves them draft-only.
    pub fn propose_policy_pack(
        &self,
        pack_id: &str,
        version: &str,
        constitution_version: Option<&str>,
    ) -> Result<PackProposalBatch> {
        let active;
        let context = match constitution_version {
            Some(version) => Some(version),
            None => {
                active = self.store.active_constitution_version()?;
                active.as_ref().map(|version| version.id.as_str())
            }
        };
        self.store
            .propose_policy_pack(pack_id, version, context, now_unix())
    }

    /// Register and propose the five Common Core principles through exactly the
    /// same immutable-pack path used by extension packs.
    pub fn propose_common_core(&self) -> Result<PackProposalBatch> {
        let pack = common_core_pack();
        self.register_policy_pack(&pack)?;
        self.propose_policy_pack(&pack.id, &pack.version, None)
    }

    /// Register and propose the optional `fleet-delivery` pack (ADR-0034):
    /// review, publication, commit identity, scope, and freshness.
    ///
    /// Uses the same immutable-pack path as every other pack, so shipping these
    /// clauses is not enforcement — each still needs an explicit adopt, tailor,
    /// or reject before it governs anything.
    pub fn propose_fleet_delivery(&self) -> Result<PackProposalBatch> {
        let pack = fleet_delivery_pack();
        self.register_policy_pack(&pack)?;
        self.propose_policy_pack(&pack.id, &pack.version, None)
    }
}
