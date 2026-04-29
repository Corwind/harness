# Harness

A native macOS harness for LLMs. Provider-agnostic. SwiftUI front, Rust back.

## What it is

Harness is a desktop chat application for large language models. The interface
is a native macOS SwiftUI app; the backend is a Rust sidecar that the app
spawns at startup. The sidecar exposes a local HTTP+SSE API on loopback,
persists conversations to SQLite, and abstracts every LLM provider behind a
single trait so adding Gemini, Ollama, or any other provider only requires a
new adapter crate. No UI changes required.

Tools called by the model are executed under macOS `sandbox-exec` using
user-managed SBPL templates attached per conversation. The default is
fail-closed: a conversation with no template attached refuses to run external
tools.

The first shipping provider is Anthropic's Claude.

## Status

Pre-v1, under active development. Phase 0 (contracts) and Phase 1 (core
implementation) are complete. Phase 2 (UI integration) wires chat, settings,
conversation tabs, and sandbox templates end-to-end. Phase 3 covers polish,
packaging, code signing, and notarisation.

## Architecture

Both halves of the system follow hexagonal architecture (ports & adapters).

**Backend (Rust).** `harness-core` is the domain and ports — pure types,
errors, and `async-trait` definitions, with no I/O and no workspace
dependencies. Adapter crates (`harness-storage`, `harness-sandbox`,
`harness-providers-*`, `harness-tools`) depend on `harness-core` and
implement its ports. `harness-orchestrator` is the application layer; it
depends on ports only. `harness-server` is the composition root that wires
every adapter into the orchestrator and serves the HTTP+SSE API.

**Frontend (SwiftUI).** `Domain/` holds pure types and gateway protocols.
`Application/` holds `@Observable` view models that depend only on the
gateway protocols. `Adapters/` provides the concrete HTTP+SSE client, the
sidecar launcher, and the Keychain integration. `UI/` holds the SwiftUI
views, organised by feature. `App/` is the composition root that builds the
adapters and injects them into the view models.

**Communication.** The Swift app spawns the Rust binary, reads
`{"port":N,"token":"..."}` from its stdout, and issues authenticated
requests using the `X-Harness-Token` header. Streaming (LLM tokens, tool
execution events) flows over `text/event-stream`. Cancellation and
`Last-Event-ID` resume are first-class.

## Repository layout

```
backend/
  crates/
    harness-core/              ports and domain types
    harness-server/            HTTP+SSE server, composition root
    harness-storage/           SQLite adapter
    harness-orchestrator/      conversation run loop
    harness-tools/             built-in tools
    harness-sandbox/           sandbox-exec wrapping
    harness-providers/         provider registry
    harness-providers-claude/  Anthropic Claude adapter
spec/
  api.openapi.yaml             HTTP API contract
  storage-schema.sql           canonical SQLite schema
macos/
  Sources/Harness/
    Domain/                    ports and entities
    Application/               view models
    Adapters/Backend/          HTTP+SSE client and sidecar launcher
    UI/                        SwiftUI views by feature
    App/                       composition root
```

## Building

Prerequisites: a stable Rust toolchain and Xcode 15+ with the macOS 14+ SDK.

```sh
make build       # cargo build + swift build
make test        # full workspace tests
make backend     # cargo run -p harness-server (dev)
```

The macOS application bundle is produced via `xcodegen` + `xcodebuild`. See
`scripts/` for the production build pipeline.

## Running the backend in development

The Rust sidecar requires two environment variables:

```sh
export HARNESS_DB_PATH=/tmp/harness-dev.sqlite
export HARNESS_DB_KEY_HEX=$(openssl rand -hex 32)
make backend
```

The first line of stdout will be the handshake JSON. Other diagnostics go to
stderr.

For deterministic e2e tests, set `HARNESS_FAKE_PROVIDER=1` to swap the real
Claude adapter for a scripted fake.

When the macOS app spawns the sidecar, it auto-derives both env vars: the
encryption key is generated on first launch and stored in the macOS keychain
under service `com.harness.encryption-key`, and the DB path defaults to
`~/Library/Application Support/Harness/harness.sqlite`. Setting
`HARNESS_DB_KEY_HEX` and/or `HARNESS_DB_PATH` in the parent process's env
overrides these defaults — the keychain is only consulted when the env var
is absent. This is how `LiveBackendHarness` and other tests bypass the
keychain entirely.

## License

Dual-licensed under the [MIT License](https://opensource.org/licenses/MIT) and
the [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0)
at your option.
