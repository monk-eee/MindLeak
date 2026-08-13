- **Fixed:** an Ackplane enrolment request now carries the node's public key,
  not just its fingerprint. ADR-0085 decision 2 requires that *"only the public
  key and fingerprint enter an enrolment request"* and decision 5 requires
  Ackplane to verify the activation signature — but the shipped contract sent
  only `public_key_fingerprint`, and a fingerprint is a hash of a key rather
  than a key. The only message anywhere carrying key bytes was
  `KeyRotationRequest.successor_public_key`, which presupposes an already
  enrolled key to rotate *from*, so the chain had no origin and no signature in
  the system could ever have been verified. `EnrollmentRequest` gains
  `bytes public_key = 10`, an additive proto3 field that leaves every existing
  encoding valid. The fingerprint stays, because ADR-0085 decision 4 has an
  administrator approve *that*: a person can compare a short fingerprint across
  two screens where they cannot compare a key. Activation deliberately does not
  carry the key — approval binds to the fingerprint, so a proof permitted to
  send its own key could present a different one after approval and be verified
  against it. Closes
  `gaps.d/the-enrolment-contract-omits-the-public-key.md`.
