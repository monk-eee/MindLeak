- **Agents are told about the delivery queue, and told not to fight it.**
  `AGENTS.md` gained a git-discipline rule: arm a pull request and leave it
  alone. The queue (ADR-0062) only removes contention if agents stop refreshing
  their own branches — if each one runs `gh pr update-branch` the moment it goes
  behind, they collide continuously and nothing drains, which is how eleven
  armed, green pull requests once sat unmerged for two hours. Merging `main` in
  by hand is now reserved for the one case the queue hands back: a real
  conflict. `scripts/delivery-queue.mjs --help` explains the same thing at the
  point of use.
