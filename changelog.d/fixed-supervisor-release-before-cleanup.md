- **Worker cleanup waits for lease release:** completed workers now keep their
  session and recovery marker while a release RPC remains unconfirmed, retrying
  through the existing cleanup path. Slots rotate only after lease cleanup and
  receipt acknowledgement both finish. Terminal workers are not re-announced as
  started during retries; shutdown retains its bounded deadline and recovery
  evidence on failure.
