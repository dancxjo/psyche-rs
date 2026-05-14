# psycheOS 🧠🚀

**psycheOS** is a narrative-driven, modular cognitive operating system designed for autonomous agents. Inspired by slow, introspective cognition—like that of an isolated space probe—it processes time-sequenced sensations into structured memory and behavior through layered distillation.

psycheOS intersects with [Project PETE](https://dancxjo.github.io/project-pete.html): the Pseudoconscious Experiment in Thought and Emotions. PETE explores embodied pseudoconscious agents, while psycheOS generalizes the slower narrative-cognition substrate that several PETE experiments depend on.

## Overview

psycheOS is not a conventional OS. It’s a cognitive architecture built on top of a minimal Linux environment that uses:

- 🧱 **Sensation logs**: Raw data input (e.g. vision, audio, telemetry) stored chronologically.
- 🌀 **Wits**: Modular cognitive layers that compress and interpret sensations into:
  - **Instants** → **Situations** → **Episodes** → **Narratives**
- 🧬 **Memory layer**: Shared bus backed by:
  - **Neo4j** for symbolic/graph memory
  - **Qdrant** for vector-based semantic memory
- 💬 **LLM services**: Modular AI daemons for embedding, chatting, and instruction parsing.
- 🔌 **Unix socket interface**: Sensor input and motor output via `/run/quick.sock`, adhering to a simple natural-language-over-path protocol.

## Project Goals

- Model cognition as a **chronological narrative** rather than reactive pipelines.
- Support **modular, slow, meaningful cognition** with variable priority and rhythm.
- Enable both **robotic** and **virtual agents** to introspect and act deliberately.
- Allow **declarative configuration** of bodies and brains using `psyche.toml`.

## Getting Started

### Requirements

- Linux (Alpine preferred for boot images)
- Rust (latest stable)
- Podman or Docker (for embedding services)
- Neo4j and Qdrant running locally or in containers

### Setup

```bash
# Clone repository
git clone https://github.com/dancxjo/psyche-rs.git
cd psyche-rs

# Build
cargo build

# Run the core orchestrator daemon
sudo ./target/debug/psyched \
  --log-level info \
  --qdrant-url http://localhost:6333 \
  --neo4j-url bolt://localhost:7687 \
  --neo4j-user neo4j --neo4j-pass password
````

Optionally start services:

```bash
# Start memory services (adjust for Docker/Podman)
podman-compose up -d neo4j qdrant ollama
# Start audio transcription daemon
whisperd gen-systemd > /etc/systemd/system/whisperd.service
sudo systemctl daemon-reexec
sudo systemctl enable --now whisperd
```
GPU acceleration is enabled by default. Add `--no-gpu` to the `ExecStart` line
if you want to force CPU-only transcription.

### Unix Socket Input

You can send input to the core daemon like so:

```bash
echo -e "/vision\nI see a red light blinking in the distance.\n.\n" | socat - UNIX-CONNECT:/run/quick.sock
```

### Stream Timing

Timestamps now start when the first audio or image segment is received. Streams no longer need a `@{timestamp}` prefix.

`whisperd` limits queued segments to avoid runaway memory use. If transcriptions fall behind the newest segments are favored.

## Architecture

```
┌────────────┐      ┌────────────┐
│  Sensor(s) │──▶──▶│   psyched  │──▶──▶ Motor(s)
└────────────┘      └─────┬──────┘
                          │
         ┌────────────────┴────────────────┐
         │              Wits              │
         │                                  │
         │  Instant ▶ Situation ▶ Episode   │
         └────────────────┬────────────────┘
                          ▼
               ┌────────────────────┐
               │     Memory Layer   │
               │  (Neo4j + Qdrant)  │
               └────────────────────┘
```

Each *Wit* is a modular wit defined declaratively and run by `psyched`.
Pipeline sections can include a `feedback` field naming another Wit. When set,
the originating Wit’s output is stored under the target Wit’s input kind so it
can immediately act on that text.

## Philosophy

* **Sentences are the atoms of meaning.**
* **Time is the substrate of cognition.**
* **Compression is understanding.**
* **Memory is the bus.**
* **Slowness is a feature, not a bug.**

## Related Projects

* [Project PETE](https://dancxjo.github.io/project-pete.html): Pseudoconscious Experiment in Thought and Emotions.
* [`psyche`](./psyche): Core abstractions for sensations, memory, and cognition.
* [`layka`](./soul): The default soul for a long-term autonomous persona.

## License

MIT or Apache 2.0, at your discretion.

---

> “You are Layka, an autonomous space probe running psycheOS…”
> — *the first line of your mission log*

