# AGENT Instructions
- Always run `cargo test` after modifications.
- Keep commit messages concise.
- Provide clear, insightful inline documentation with examples and doctests when possible.
- Prefer BDD/TDD style tests.
- When creating files via shell heredocs, use quotes to avoid stray temp files.
- Update `constraints.cypher` whenever Memory types change.
- GitHub Actions workflow runs `cargo test` using `actions-rs/toolchain@v1` with caching.
- When adding async traits that don't need `Send`, annotate with `#[async_trait(?Send)]`.
- If running `rustfmt` touches unrelated files, it's fine to keep those changes.
- Run `cargo fmt` before committing changes.
- Instrument code with tracing logs at appropriate levels.
- Emit `trace` level logs for all LLM prompts and streaming token events.
- Log the complete response from every LLM call at `debug` level.
- When implementing test memory stores that persist `Memory::Of`, clone the inner
  value before saving to prevent panics when cloning.
- Ensure every async call is awaited unless intentionally detached with
  `tokio::spawn`.
- Keep narrative prompt text under `daringsby/src` and pass it into library
  constructors instead of embedding it in `psyche-rs`.
- Use `httpmock` for HTTP-based tests to avoid external network dependencies.
- Set `USE_MOCK_LLM=1` in tests to force Ollama-backed wits to use `MockChat`.
- Keep `.rs` files focused. Create a new source file for each new type and split
  large modules like `lib.rs` into smaller pieces.
- Provide mutable `set_*` variants when adding builder methods to wrappers.
- Use the `LLMClient` trait for streaming LLM interactions.
- Avoid streaming silence frames when audio is not playing.
- Format `Impression` values using their `how` string when included in prompts
  instead of serializing the entire struct.
- Stream HTTP responses using `bytes_stream` to avoid blocking when servers
    stream data chunked.
- Use `src/test_helpers.rs` for shared test utilities like `StaticLLM`,
  `TestSensor` and `TwoBatch`.
- Doc tests must not depend on modules behind `#[cfg(test)]`.
- For async doc tests, use `tokio_test::block_on` to avoid bringing in runtime macros.
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
- Keep `WillRuntimeConfig` in sync with fields used by `Will` runtimes.
- Log all memory store errors at `error` level.
- Refer to external speakers as "my interlocutor" in prompts and logs.
- Pass interlocutor utterances to the voice module as plain text without
  wrapping them in narrative phrasing.
- Remove obsolete feature flags when the codebase no longer relies on them.
- Use `tracing-test` with the `no-env-filter` feature when verifying log output.
- Ensure `persist_impression` checks for existing sensations via `find_sensation` to avoid duplicates.
- When mapping Neo4j nodes to structs, alias the `uuid` property to the `id` field.
- Link summary impressions to originals using a :SUMMARIZES relationship in Neo4j.
- Forward `info`, `warn`, and `error` logs to the Combobulator as
  `log.system` sensations using `LogSensationLayer`.
- Prefer `smartcore` over `linfa` for ML tasks to avoid heavy dependencies.
- Gate heavy dependencies like OpenCV behind optional Cargo features to keep
  compile times manageable.
- When OpenCV isn't available, run tests with `--no-default-features`.
- Follow the psyche-os directory structure in README.
- Run `cargo test --workspace` before committing.
- Set `NASA_API_KEY` when using rover data scripts to avoid API rate limits.
- When adding new daemons, list them in `Cargo.toml` and provide matching
  executable scripts in `scripts/` without the `.sh` extension.
- When showing long download progress in CLI tools, direct `indicatif`
  progress bars to stderr to avoid polluting captured stdout in tests.
- Maintain project documentation under the `docs/` directory.
- When working with `webrtc_vad` in async tests, use `spawn_local` since its
  context isn't `Send`.
- Set `WHISPER_SEGMENTS_DIR` to dump each PCM segment as a WAV file during tests
  or debugging.
- For daemon tests expecting streamed output, use `BufReader::read_line` to
  avoid hanging when connections stay open.
- Store soul configuration in a single `identity.toml` that includes sensor and
  motor settings; update scripts and loaders accordingly.
- Add tests when introducing new RPC methods.
