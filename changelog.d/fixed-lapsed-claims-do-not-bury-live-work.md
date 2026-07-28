- **A lapsed claim no longer buries the work someone is actually holding.** The
  Work board ranked rows by stored status, so a claim whose lease expired nine
  hours ago sorted identically to one being worked on right now. One session
  left fifteen such rows behind in a day and the board became unreadable: three
  live tasks scattered among twenty-five dead ones, distinguishable only by
  reading a timestamp on every row. An expired claim is claimable — the store's
  compare-and-swap already admits `status = 'claimed' AND lease_expires_at <
  now`, and the row already described itself as "Claim expired · Ready" — so it
  now ranks as ready work, below tasks nobody has started, and live claims sort
  to the top where they belong. Nothing is reaped, rewritten or transitioned to
  achieve it: expiry is a function of `lease_expires_at` and the clock, derived
  at render time the way effective edge weight is derived at query time.
