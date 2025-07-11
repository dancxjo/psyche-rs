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

The web interface is served over **HTTPS** on the chosen host and port.

Available options (see `main.rs`):

* `--quick-url`: Base URL for quick tasks (default: `http://localhost:11434`)
* `--combob-url`: Base URL for Combobulator tasks (default: `http://localhost:11434`)
* `--will-url`: Base URL for Will tasks (default: `http://localhost:11434`)
* `--memory-url`: Base URL for memory operations (default: `http://localhost:11434`)
* `--quick-model`: Model used for Quick tasks (default: `gemma3n`)
* `--combob-model`: Model used for Combobulator tasks (default: `gemma3n`)
* `--will-model`: Model used for Will tasks (default: `gemma3n`)
* `--memory-model`: Model used for memory operations (default: `gemma3n`)
* `--voice-url`: Dedicated Ollama base URL for the voice loop (default: `http://localhost:11434`)
* `--voice-model`: Model used for the voice loop (default: `gemma3n`)
* `--embedding-model`: Model used for embeddings (default: `nomic-embed-text`)
* `--tts-url`: Coqui TTS base URL (default: `http://localhost:5002`)
* `--language-id`: Language identifier for TTS (optional)
* `--speaker-id`: Speaker ID for TTS (default: `p234`)
* `--tls-cert`: TLS certificate path (default: `cert.pem`, auto-generated if missing)
* `--tls-key`: TLS private key path (default: `key.pem`, auto-generated if missing)

`daringsby` feeds raw sensations directly into the Combobulator, which summarizes them into moments. Moments loop back through the Combobulator so higher level impressions continue to build over time.

### Supervising genii

`PsycheSupervisor` keeps multiple genii running and restarts them if one
crashes. Add each `Genius` via [`add_genius`] and call [`start`] to spawn
threads. Use [`shutdown`] to stop all genii:

```no_run
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use psyche_rs::{PsycheSupervisor, QuickGenius};

let (_tx, rx) = unbounded_channel();
let (out_tx, _out_rx) = unbounded_channel();
let quick = Arc::new(QuickGenius::new(rx, out_tx));

let mut sup = PsycheSupervisor::new();
sup.add_genius(quick);
sup.start(None);
sup.shutdown().await;
```

### Thread-local clients

Each genius thread constructs a [`ThreadLocalContext`] holding an LLM client and
memory store for that thread. This keeps network handles isolated and avoids
cross-thread locking.

```rust
use psyche_rs::{ThreadLocalContext, InMemoryStore, OllamaLLM};
use std::sync::Arc;

let ctx = ThreadLocalContext {
    llm: Arc::new(OllamaLLM::default()),
    store: Arc::new(InMemoryStore::new()),
};
```

### Thread management and configuration

Pin each genius to a CPU core and configure its clients separately.

```rust
use psyche_rs::{PsycheSupervisor, QuickGenius, Wit, Will, InMemoryStore, OllamaLLM};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;

let (out_tx, _out_rx) = unbounded_channel();
let (quick, _tx) = QuickGenius::with_capacity(1, out_tx);
let quick = Arc::new(quick);

let llm = Arc::new(OllamaLLM::default());
let store = Arc::new(InMemoryStore::new());
let wit = Wit::new(llm.clone()).memory_store(store.clone());
let will = Will::new(llm.clone()).memory_store(store);

let mut sup = PsycheSupervisor::new();
sup.add_genius_on_core(quick, Some(0));
sup.start(None);
```

---

## 🌌 WebXR Memory Visualization

The `daringsby` server exposes a VR scene showing recent memories as nodes in 3D space.

1. Run `project_embeddings.py` to project Qdrant embeddings and store their 3D coordinates:

   ```bash
   python scripts/project_embeddings.py \
     --qdrant http://localhost:6333 \
     --neo4j http://localhost:7474 \
     --neo-user neo4j --neo-pass password
   ```

   This generates `embedding_positions.json` and updates Neo4j nodes with `embedding3d` data.

2. Start `daringsby` and open `https://<host>:<port>/memory_viz.html` in a WebXR-capable browser.
   Use a headset or mouse to explore the grid of memories.




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