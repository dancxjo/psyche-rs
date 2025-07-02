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
- Instrument code with tracing logs at appropriate levels.
- Emit `trace` level logs for all LLM prompts and streaming token events.
- When implementing test memory stores that persist `Memory::Of`, clone the inner
  value before saving to prevent panics when cloning.
- Ensure every async call is awaited unless intentionally detached with
  `tokio::spawn`.
- Keep narrative prompt text under `daringsby/src` and pass it into library
  constructors instead of embedding it in `psyche-rs`.
- Use `httpmock` for HTTP-based tests to avoid external network dependencies.
- Keep `.rs` files focused. Create a new source file for each new type and split
  large modules like `lib.rs` into smaller pieces.
- Use the `LLMClient` trait for streaming LLM interactions.
- Avoid streaming silence frames when audio is not playing.
- Format `Impression` values using their `how` string when included in prompts
  instead of serializing the entire struct.
- Stream HTTP responses using `bytes_stream` to avoid blocking when servers
    stream data chunked.
- Use `src/test_helpers.rs` for shared test utilities like `StaticLLM`,
  `TestSensor` and `TwoBatch`.
- Avoid panicking when sensors are reused; prefer returning an empty stream or
  clearly documenting single-use behavior.
- When modifying canvas-related code, run `cargo test --workspace` to verify
  canvas and SVG motors.
- Use `Will::thoughts` to broadcast reasoning as sensations when needed.
- When adding new sensors or motors, be sure to register them in `daringsby/src/main.rs`
  and extend the dispatch logic in `drive_will_stream`.
- Comment out or ignore tests that consistently hang for over 60s.
- When spawning long-running tasks, keep the `JoinHandle` and abort it on
  shutdown to ensure HTTP connections close quickly.
- When throttling duplicate Will snapshots, hash the serialized snapshot and
  skip LLM calls within `min_llm_interval_ms`; ensure tests cover this logic.
