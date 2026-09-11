- **Peer cleanup after a worker-slot failure:** a fatal slot error now signals
  the remaining slots through their normal shutdown path before returning the
  original failure. Healthy peers stop their processes, release leases and flush
  receipts instead of being abruptly cancelled. Cleanup is bounded to thirty
  seconds; failed or unfinished slots retain their existing recovery evidence.
  No automatic restart or process-loss recovery is implied.
