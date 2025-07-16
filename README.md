# Psyche OS

This repository contains the "psyche-os" workspace. It provides a clean
separation between core cognitive logic, configurable memory backends, and
deployment artifacts.

```
psyche-os/
├── psyche/                     # Core library
│   ├── src/
│   ├── tests/
│   └── features/               # Gherkin specs
├── psyched/                    # Main daemon
│   ├── src/
│   └── config/
├── containers/                 # Docker/Podman support
│   ├── alpine-bootstick/
│   └── dev/
├── soul/                       # Instance-specific identity and memory
├── tests/                      # Integration and scenario tests
├── scripts/                    # Helper scripts
├── Cargo.toml                  # Workspace root
└── README.md
```
