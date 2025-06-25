# AGENT Instructions
- Always run `cargo test` after modifications.
- Keep commit messages concise.
- Provide clear, insightful inline documentation with examples and doctests when possible.
- Prefer BDD/TDD style tests.
- Update `constraints.cypher` whenever Memory types change.
- GitHub Actions workflow runs `cargo test` using `actions-rs/toolchain@v1` with caching.
- When adding async traits that don't need `Send`, annotate with `#[async_trait(?Send)]`.
- If running `rustfmt` touches unrelated files, it's fine to keep those changes.
- Run `cargo fmt` before committing changes.
