- The Bridge repository timeline now flags an event whose signing key has
  since been revoked, judged as of now rather than as of when the event was
  accepted (ADR-0084 decision 12). The underlying ledger record is never
  altered; only the timeline's own derived annotation can change as a key's
  lifecycle changes.
