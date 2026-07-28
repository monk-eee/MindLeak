- **The ADR files and the design ledger agree again.** `make design-audit`
  reported seven items of drift: five ADRs the ledger had never seen (0054,
  0055, 0060, 0061, 0062) and two whose file said `Proposed` while the ledger
  recorded them accepted by a person (0057, 0058). An unregistered ADR is a
  decision the ledger cannot reason about — it never appears on the design
  board and cannot be reconciled — and a file that disagrees with the ledger
  means one of the two is lying about whether a decision was made. All five are
  registered and the two status lines now match the ledger. Two remain
  deliberately unresolved: 0054 and 0055 claim `Accepted` in their files with no
  decision recorded, and inventing a decider for them is exactly what the check
  exists to prevent.
