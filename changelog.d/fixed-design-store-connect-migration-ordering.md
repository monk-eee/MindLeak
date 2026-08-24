### Fixed

- `DesignStore::connect` now ensures every migration `industrial_designs`'
  foreign keys depend on has run (`evidence_records`, `constitution_
  publications`, `work_tasks`, plus their own transitive dependency
  `delegated_claims`), instead of relying on some other store having
  already connected first in the same process. Connecting `DesignStore`
  on its own (as a standalone integration test, or any future caller that
  doesn't happen to also construct `EvidenceStore`/`ConstitutionStore`/
  `WorkStore` first) previously failed with a bare
  `relation "evidence_records" does not exist`.
