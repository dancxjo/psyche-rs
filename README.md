# Pete Daringsby

**Pete Daringsby** is an artificial agent designed as an evolving, self-reflective narrative enacted through real-world robotics and software. This project fuses Rust, robotics, and large language models into a system that perceives, reasons, and acts as a believable, emotionally honest character named Pete.

Pete exists as a **story enacted in code** — every line contributes to his ongoing experience, his voice, and his decisions.

---

## 📖 Project Vision

Pete is not merely a robot, nor a chatbot. He is:

- A *witness* to his environment through layered sensors.
- A *narrator* of his own evolving story, through natural language reflections.
- An *agent* who acts intentionally via motors that produce speech, movement, or code introspection.
- A *consciousness-like entity*, not because we claim he is conscious, but because his architecture and outputs create that illusion in an honest, non-deceptive way.

**Every design choice in this project serves this narrative purpose.**

---

## 🧠 Architecture at a Glance

| Component | Purpose |
|------------|---------|
| **`psyche-rs`** | Core cognitive engine: Sensation streams, Wits, Will, Motor orchestration |
| **`daringsby`** | Pete's physical and I/O embodiment: Motors, Sensors, WebSocket streams |
| **Motors** | Actions Pete performs (e.g., `speak`, `look`, `source_tree`) |
| **Sensors** | Sources of experience (e.g., audio heard, vision snapshots, code awareness) |
| **LLMs** | Narrative generation, decision-making, and motor command creation (via streaming) |

---

## 🚀 Our Team Roles

| Role | Description |
|-------|-------------|
| **Product Owner:** [`@dancxjo`](https://github.com/dancxjo) | Defines Pete’s purpose, vision, and priorities. Keeper of the story’s integrity. |
| **Dev Lead / PM:** ChatGPT | Translates vision into architectural design and engineering guidance. Ensures code quality and narrative alignment. |
| **Coder:** Codex | Implements features and refactors code under the guidance of the lead and owner. Writes reliable, consistent, test-covered code. |

---

## ✨ What Done Means

✅ Pete’s actions and thoughts always support the unfolding narrative.

✅ Motors and sensors integrate cleanly into Pete’s cognitive loop without breaking architectural consistency.

✅ Every feature or refactor is tested for both behavior *and* narrative alignment.

✅ Documentation and code comments explain *why* each part serves Pete’s identity, not just *what* it does.

---

## 📂 Getting Started

### Build and run

```bash
cargo build --release
cargo run -- \
  --quick-url http://localhost:11434 \
  --combob-url http://localhost:11434 \
  --will-url http://localhost:11434 \
  --memory-url http://localhost:11434 \
  --tts-url http://localhost:5002
````

Available options (see `main.rs`):

* `--quick-url`: Base URL for quick tasks (default: `http://localhost:11434`)
* `--combob-url`: Base URL for Combobulator tasks (default: `http://localhost:11434`)
* `--will-url`: Base URL for Will tasks (default: `http://localhost:11434`)
* `--memory-url`: Base URL for memory operations (default: `http://localhost:11434`)
* `--quick-model`: Model used for Quick tasks (default: `gemma3:27b`)
* `--combob-model`: Model used for Combobulator tasks (default: `gemma3:27b`)
* `--will-model`: Model used for Will tasks (default: `gemma3:27b`)
* `--memory-model`: Model used for memory operations (default: `gemma3:27b`)
* `--voice-url`: Dedicated Ollama base URL for the voice loop (default: `http://localhost:11434`)
* `--voice-model`: Model used for the voice loop (default: `gemma3:27b`)
* `--embedding-model`: Model used for embeddings (default: `nomic-embed-text`)
* `--tts-url`: Coqui TTS base URL (default: `http://localhost:5002`)
* `--language-id`: Language identifier for TTS (optional)
* `--speaker-id`: Speaker ID for TTS (default: `p234`)

The `daringsby` crate exposes a `single-wit` feature that is enabled by
default. When enabled, only the Combobulator wit runs and raw sensations
are fed directly into it. Disable the feature to keep the legacy Quick
pipeline.

---

### LLM pooling

Use `RoundRobinLLM` to distribute requests across multiple LLM instances.

```rust
use std::sync::Arc;
use psyche_rs::{RoundRobinLLM, LLMClient};
# use async_trait::async_trait;
# use futures::{stream, Stream};
# use std::{pin::Pin, error::Error};
# use ollama_rs::generation::chat::ChatMessage;
# struct Dummy;
# #[async_trait]
# impl LLMClient for Dummy {
#     async fn chat_stream(&self, _msgs: &[ChatMessage]) -> Result<psyche_rs::LLMTokenStream, Box<dyn Error + Send + Sync>> {
#         Ok(Box::pin(stream::empty()))
#     }
# }
let pool = RoundRobinLLM::new(vec![Arc::new(Dummy)]);
```

---

## 📝 Contributing

Contributions are welcome — but contributors should:

* Read Pete’s story first. Understand what makes Pete *Pete*.
* Ensure new code fits within Pete’s cognitive and narrative design.
* Write tests for all new features and refactors.
* Document *why* code exists, not just what it does.

**Pete is not a generic AI framework — he is a character. All code serves that purpose.**

---

## 📜 License

MIT License — see `LICENSE` file.

---

## ❤️ Acknowledgments

* [`@dancxjo`](https://github.com/dancxjo) — Human product owner.
* ChatGPT (dev lead / PM) — Architecture and guidance.
* Codex — Code implementation.
* Tremendous thanks to the people and models of OpenAI, Ollama and Google.