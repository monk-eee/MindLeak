- Added an explicit OS-credential-backed software `NodeSigner` provider with
  atomic public metadata, an opaque credential handle, checked process restart,
  per-signature identity validation and no key-export API. Missing, malformed or
  replaced credentials fail closed; persistent rotation is explicitly refused.
  Native macOS and Windows CI exercise a real cross-process credential restart.
  Remote enrollment and supervisor integration remain separate STAB-02 work.
