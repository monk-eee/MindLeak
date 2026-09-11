- **Stopping an exited worker reported EPERM and left its lease held on macOS.** While developing `a_failed_slot_retries_lease_release_without_losing_rejected_evidence` in [multi_agent_end_to_end.rs](../crates/ackplane-supervisor/tests/multi_agent_end_to_end.rs), two five-second release retry waits delayed peer shutdown until [prompt_worker](../crates/ackplane-supervisor/src/bin/prompt_worker.rs)'s ten-second gate deadline. Two runs then reported `Operation not permitted (os error 1)` from [WorkerProcess::stop](../crates/ackplane-supervisor/src/worker_adapter.rs), wrapped misleadingly as `SpawnFailed`, and the peer lease remained held without terminal delivery. Signalling peers before retries fixes that delayed-shutdown regression, but the OS refusal itself is unexplained and left for investigation. Add a focused already-exited process-group regression and distinguish stop errors from spawn errors; do not treat EPERM as proof that a group is gone.

	The same cleanup diagnostic recurred at `2026-09-11T01:57:27.501703Z` during
	`cargo test --locked -p ackplane-supervisor --quiet` with the spawn/evidence
	ordering fix on top of `856a068a`. All 75 supervisor tests passed, including
	the targeted persistence assertions, but the concurrent fault-test logs also
	reported that a slot's cleanup stopped with EPERM. The suite's green result
	therefore does not qualify this separate stop-error path, and the observation
	is not limited to the earlier delayed-peer implementation. Left unresolved.
