# psycheOS 🧠🚀

**psycheOS** is a narrative-driven, modular cognitive operating system designed for autonomous agents. Inspired by slow, introspective cognition—like that of an isolated space probe—it processes time-sequenced sensations into structured memory and behavior through layered distillation.

## Overview

psycheOS is not a conventional OS. It’s a cognitive architecture built on top of a minimal Linux environment that uses:

- 🧱 **Sensation logs**: Raw data input (e.g. vision, audio, telemetry) stored chronologically.
- 🌀 **Distillers (Wits)**: Modular cognitive layers that compress and interpret sensations into:
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
sudo ./target/debug/psyched
````

Optionally start services:

```bash
# Start memory services (adjust for Docker/Podman)
podman-compose up -d neo4j qdrant ollama
```

### Unix Socket Input

You can send input to the core daemon like so:

```bash
echo -e "/vision\nI see a red light blinking in the distance.\n.\n" | socat - UNIX-CONNECT:/run/quick.sock
```

## Architecture

```
┌────────────┐      ┌────────────┐
│  Sensor(s) │──▶──▶│   psyched  │──▶──▶ Motor(s)
└────────────┘      └─────┬──────┘
                          │
         ┌────────────────┴────────────────┐
         │         Distillers (Wits)       │
         │                                  │
         │  Instant ▶ Situation ▶ Episode   │
         └────────────────┬────────────────┘
                          ▼
               ┌────────────────────┐
               │     Memory Layer   │
               │  (Neo4j + Qdrant)  │
               └────────────────────┘
```

Each *Wit* is a modular distiller defined declaratively and run by `psyched`.

## Philosophy

* **Sentences are the atoms of meaning.**
* **Time is the substrate of cognition.**
* **Compression is understanding.**
* **Memory is the bus.**
* **Slowness is a feature, not a bug.**

## Related Projects

* [`psyche`](./psyche): Core abstractions for sensations, memory, and cognition.
* [`layka`](./soul): The default soul for a long-term autonomous persona.

## License

MIT or Apache 2.0, at your discretion.

---

> “You are Layka, an autonomous space probe running psycheOS…”
> — *the first line of your mission log*

