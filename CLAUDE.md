# DiffScope - AI Code Review Engine

## Quick Reference
```bash
cargo build               # Build
cargo test                # Run tests
cargo run -- --help       # CLI usage
```

## Architecture
- **Language**: Rust
- **Database**: PostgreSQL (via sqlx with compile-time checked queries)
- **AI providers**: Multi-model support (Anthropic primary, OpenAI, OpenRouter)
- **Deployment**: Docker → k3s via Helm chart in `charts/`
- **GitHub integration**: GitHub App auth (not PATs)

## Key Directories
- `src/` — Core analysis engine, CLI, API
- `charts/` — Helm chart for k8s deployment
- `migrations/` — PostgreSQL migrations (sqlx)
- `eval/` — Evaluation and benchmarking
- `examples/` — Usage examples

## Conventions
- Use frontier models for reviews — never default to smaller models
- Vault integration for secrets management (HashiCorp Vault)
- GitHub App authentication over personal access tokens
- Wide events for observability (OpenTelemetry-compatible)
- Self-hosted first: Ollama/vLLM/LM Studio should be first-class providers
