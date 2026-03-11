## Development Workflow

After making any code changes, always run the following commands and fix any
issues before considering the work complete:
```bash
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo test --all
```

- Run `cargo fmt` first as it may change files that affect subsequent steps
- All Clippy warnings are treated as errors — fix them, don't suppress them
- All tests must pass before changes are final
