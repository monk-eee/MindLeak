- Extreme persisted edge timestamps no longer overflow the signal
  reinforcement-span calculation before graph reads. Saturating subtraction
  prevents debug panics and release-mode wrapping; reversed timestamps retain
  a zero span. Normal reinforcement thresholds, stored timestamps, and decay
  behavior are unchanged.
