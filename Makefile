# MindLeak developer commands. On Windows, run the underlying commands directly
# (see DEVELOPERS.md) if `make` is unavailable.

.PHONY: setup build test bench lint fmt fmt-check clippy run ext-install ext-compile ext-lint ext-test ci

setup: ## Install pre-commit hooks and extension deps
	pip install pre-commit
	pre-commit install
	pre-commit install --hook-type pre-push
	npm --prefix editors/vscode install

build: ## Build the workspace (debug)
	cargo build

test: ## Run the Rust test suite
	cargo test --all

fmt: ## Format Rust code
	cargo fmt --all

fmt-check: ## Check Rust formatting
	cargo fmt --all -- --check

clippy: ## Lint Rust with clippy (warnings = errors)
	cargo clippy --all-targets --all-features -- -D warnings

lint: fmt-check clippy ext-lint ## Run all linters

run: ## Build and run the MCP server
	cargo run -p mindleak-mcp

bench: ## Run experiments (impact-precision + four-arm agent-outcome)
	cargo build -p mindleak-mcp
	node scripts/experiments/impact-vs-similarity.mjs
	node scripts/experiments/agent-outcome-benchmark.mjs

ext-install: ## Install VS Code extension dependencies
	npm --prefix editors/vscode install

ext-compile: ## Compile the VS Code extension
	npm --prefix editors/vscode run compile

ext-lint: ## Lint the VS Code extension
	npm --prefix editors/vscode run lint

ext-test: ## Run the VS Code extension unit tests (vitest)
	npm --prefix editors/vscode test

ci: fmt-check clippy test ext-compile ext-lint ext-test ## Everything CI runs
