# AGENT Instructions
- Always run `cargo test` after modifications.
- Keep commit messages concise.
- Provide inline documentation with examples.
- Prefer BDD/TDD style tests.
- Update `constraints.cypher` whenever Memory types change.
- GitHub Actions workflow runs `cargo test` using `actions/setup-rust@v2` with caching.
