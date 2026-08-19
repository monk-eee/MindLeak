# MindLeak developer commands. On Windows, run the underlying commands directly
# (see DEVELOPERS.md) if `make` is unavailable.

.PHONY: setup worktree-setup install-servers adr-index changelog design-audit merge-audit queue queue-watch sweep board-health stranded-report status reingest tool-surface build test script-test ratchet coverage bench agent-bench lint fmt fmt-check clippy run ext-install ext-compile ext-lint ext-test ci

setup: ## Install pre-commit hooks and extension deps
	pip install pre-commit
	# Installs pre-commit, pre-push and post-commit together — the config
	# declares default_install_hook_types, so no hook depends on someone
	# remembering an extra flag. post-commit is the one that records evidence.
	pre-commit install --install-hooks
	cargo install cargo-llvm-cov --locked
	npm --prefix editors/vscode install

install-servers: ## Install the built MCP servers where every window can reach them (ADR-0073)
	# Every window is rooted at the worktree it edits, so the servers cannot live
	# inside any one worktree. They are installed once per machine instead, under
	# the user's home directory, which the extension prefers over a worktree build.
	cargo build --release -p mindleak-mcp -p lodestar-mcp
	node scripts/install-servers.mjs

reclaim: ## Report reclaimable worktrees, branches and build output (add ARGS=--reclaim to act)
	# Cleanup never happens on goodwill: the agent that created a worktree has
	# finished and moved on by the time it is safe to remove. Reports by default,
	# because no report can be un-deleted. ARGS="--reclaim --remote" also deletes
	# merged remote branches.
	node scripts/worktree-reclaim.mjs $(ARGS)

sweep: ## Report reclaimable build artefacts (add ARGS=--apply to act)
	# Diagnosis only. The sweep already runs continuously from the delivery
	# watcher (`make queue-watch`), which is what stops caches accumulating; this
	# answers "what would it remove, and why did it skip that one". Reports by
	# default for the same reason `reclaim` does. Both take the same lock in the
	# common Git directory, so a manual run cannot race the watcher's sweep.
	node scripts/artefact-sweep.mjs $(ARGS)

worktree-setup: ## Prepare a freshly created linked worktree (ADR-0038)
	# Hooks and cargo tools are shared through the common .git dir and the user's
	# cargo bin, so a new worktree only needs its own node_modules. Without it the
	# prettier and eslint hooks cannot run and the first push fails with a module
	# resolution error that says nothing about the real cause.
	npm --prefix editors/vscode ci

adr-index: ## Regenerate docs/adr/README.md from the ADR files
	node scripts/adr-index.mjs

changelog: ## Show what the next release would contain, from changelog.d fragments
	node scripts/changelog.mjs

gaps: ## List every known gap, from gaps.d fragments
	node scripts/gaps.mjs --list

design-audit: ## Report drift between the ADR files and the design ledger (needs a release build)
	node scripts/design-audit.mjs

merge-audit: ## Report merged branches whose commits never reached main
	node scripts/merge-audit.mjs

queue: ## Show the delivery queue and update the branch whose turn it is (ADR-0062)
	node scripts/delivery-queue.mjs

board-health: ## Separate work a human must decide from work nobody can (ADR-0058)
	node scripts/board-health.mjs

stranded-report: ## Name the likely shipping commit for each lapsed claim (ADR-0048)
	node scripts/stranded-report.mjs

status: ## Read live Lodestar/MindLeak state directly, no agent session needed
	node scripts/status.mjs

tool-surface: ## Measure what tools/list costs every session to load (ADR-0059)
	cargo build -p mindleak-mcp -p lodestar-mcp
	node scripts/measure-tool-surface.mjs

queue-watch: ## Run the delivery queue as an agent until stopped (ADR-0062)
	node scripts/delivery-queue.mjs --watch

build: ## Build the workspace (debug)
	cargo build

test: ## Run the Rust test suite
	cargo test --all

script-test: ## Run the repository's own script tests
	node scripts/script-tests.mjs

reingest: ## Re-ingest tracked files so an extractor upgrade reaches the existing graph
	node scripts/reingest.mjs

ratchet: ## Report the governed module count to the ratchet watching it
	node scripts/observe-module-length.mjs

coverage: ## Run Rust + extension tests with coverage reports
	cargo llvm-cov --workspace --all-features --lcov --output-path coverage.lcov
	cargo llvm-cov report --summary-only --fail-under-lines 80
	npm --prefix editors/vscode run test:coverage

fmt: ## Format Rust code
	cargo fmt --all

fmt-check: ## Check Rust formatting
	cargo fmt --all -- --check

clippy: ## Lint Rust with clippy (warnings = errors)
	cargo clippy --all-targets --all-features -- -D warnings

lint: fmt-check clippy ext-lint ## Run all linters

run: ## Build and run the MCP server
	cargo run -p mindleak-mcp

bench: ## Run graph, sensor, overlap, and four-arm context experiments
	cargo build -p mindleak-mcp
	npm --prefix editors/vscode run compile
	node scripts/evaluate-sensors.mjs
	node scripts/evaluate-signal.mjs
	node scripts/evaluate-handoffs.mjs
	node scripts/evaluate-overlap.mjs
	node scripts/experiments/impact-vs-similarity.mjs
	node scripts/experiments/agent-outcome-benchmark.mjs

agent-bench: ## Run the premium 12-run pinned-agent product decision gate
	cargo build -p mindleak-mcp -p lodestar-mcp
	node scripts/evaluate-agent-loop.mjs --repeats=3

ext-install: ## Install VS Code extension dependencies
	npm --prefix editors/vscode install

ext-compile: ## Compile the VS Code extension
	npm --prefix editors/vscode run compile

ext-lint: ## Lint the VS Code extension
	npm --prefix editors/vscode run lint

ext-test: ## Run the VS Code extension unit tests (vitest)
	npm --prefix editors/vscode test

adr-guard: ## Fail if any ADR is uncommitted or reachable from no remote ref
	node scripts/adr-guard.mjs

ci: fmt-check clippy test script-test ext-compile ext-lint ext-test ## Everything CI runs
