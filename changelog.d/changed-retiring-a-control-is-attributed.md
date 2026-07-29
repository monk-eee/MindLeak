- **Retiring a control is now attributed.** Standing a control down is the one
  act that reduces what a clause can enforce without changing a word of the
  clause — closer to granting a waiver than to editing a configuration file —
  and it was recorded as a bare status flip with no author. The store now keeps
  `retired_by` and `retired_at`, the tool is session-bound so the author is
  resolved from the session rather than supplied by the caller, and an
  unattributed retirement is refused outright. Controls retired before this was
  recorded carry no author: those retirements cannot be reconstructed, and
  inventing one would be worse than admitting the gap.
