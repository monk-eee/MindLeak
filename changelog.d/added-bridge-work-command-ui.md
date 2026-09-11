- **The Bridge Work page now executes authorized commands with explicit
  confirmation.** Create tasks, select advertised agent sessions, assign work,
  answer questions, and submit reviews through the existing command API.
  Additional runtime controls follow advertised capabilities. The page refreshes
  task versions, keeps previews immutable, preserves retry identities, and
  distinguishes queued delivery from applied commands and task completion.
  Technical records remain available behind disclosures. The browser uses the
  Bridge's verified principal without asking the operator for an identity token.
  The tenant-route guard now includes the Work mutation handlers as well as the
  read surface; embedded assets remain the only new static-resource exceptions.
  See [Bridge Work](../docs/BRIDGE-WORK.md) for the supported workflow and limits.
