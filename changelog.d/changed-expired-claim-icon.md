- **An expired claim no longer wears the owner's icon on the board.** The sort
  already knew a lapsed claim is ready work — the store's compare-and-swap
  admits it and the row says "Claim expired · Ready" — but every claimed row,
  live or lapsed, still drew `account`, the icon that means *someone is holding
  this*. So the one row that means "abandoned, take it" looked exactly like the
  rows that mean "hands off", and a board carrying fifteen of them read as a
  fleet at capacity when most of it was free. An expired claim now draws
  `watch`: a lease is a timer, and this one ran out. Derived from the clock at
  render time in `boardIconId`, never reaped or written back, so nothing is
  mutated to make the picture true.
