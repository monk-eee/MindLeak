# MindLeak developer commands. On Windows, run the underlying commands directly
# (see DEVELOPERS.md) if `make` is unavailable.

.PHONY: setup build test coverage bench agent-bench lint fmt fmt-check clippy run ext-install ext-compile ext-lint ext-test ci

setup: ## Install pre-commit hooks and extension deps
	pip install pre-commit
	pre-commit install
	pre-commit install --hook-type pre-push
	cargo install cargo-llvm-cov --locked
	npm --prefix editors/vscode install

build: ## Build the workspace (debug)
	cargo build

test: ## Run the Rust test suite
	cargo test --all

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

bench: ## Run graph, sensor, and four-arm context experiments
	cargo build -p mindleak-mcp
	npm --prefix editors/vscode run compile
	node scripts/evaluate-sensors.mjs
	node scripts/evaluate-signal.mjs
	node scripts/evaluate-handoffs.mjs
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

ci: fmt-check clippy test ext-compile ext-lint ext-test ## Everything CI runs
