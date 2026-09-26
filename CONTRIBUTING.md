# Contributing

Thanks for your interest in Aural.

**Bug reports and ideas are very welcome** — please open an issue. For problems with
dictation in a particular app, say which app (and version), what you expected, and what
the pill showed.

**Pull requests are not being accepted yet.** Aural is source-available under a noncommercial license
([docs/LICENSING.md](docs/LICENSING.md)); contributor terms that let the copyright holder
keep offering commercial licenses aren't in place yet. Once they are, this file will
explain how to contribute code.

## Ground rules for the project

- Local first: no telemetry, accounts or cloud audio. Network access only on explicit
  user action.
- No model weights, generated binaries, secrets or machine-specific configuration in
  the repository (CI rejects files over 5 MB).
- Every change is test-driven: a failing test first, then the code. Windows behaviour
  that can't be unit-tested gets an ignored integration test and a line in
  `docs/testing/manual.md`.
- `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `npm run check`, `npm test` and
  `cargo deny check` must pass. See [docs/building-windows.md](docs/building-windows.md).
