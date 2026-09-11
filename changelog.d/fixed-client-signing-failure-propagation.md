- Client signing and authentication now return typed provider failures instead
  of requiring infallible signature bytes. Federation refuses before opening a
  mutation RPC, and NodeSync stops before sending a challenge response. The
  persistent node provider can open the reusable NodeSync client through a
  private adapter using its recorded identity; missing or replaced credentials
  produce non-secret local errors. Thread-safe signer bounds preserve connection
  futures that can run on runtime workers. Added refusal and real-service recovery/loss
  coverage. CLI, supervisor and already-open stream lifecycle wiring remain
  separate work.
