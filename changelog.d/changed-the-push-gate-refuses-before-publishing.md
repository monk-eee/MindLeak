- **The push gate refuses before publishing instead of warning afterwards.**
  The Lodestar ledger already refused a publish it could not reach, but the
  Memory Plane was only consulted _after_ the push and could do no more than
  annotate the result as uncertifiable — by which point the branch was
  irreversibly on the remote. Both planes are now resolved before anything is
  pushed, and an unreachable Memory Plane stops the publish with a message
  naming `MINDLEAK_MCP_BIN` and the build command. A warning issued after an
  irreversible act was never the same protection as a refusal before it.
