# Contributing

Thanks for your interest in Aural.

**Bug reports and ideas are very welcome** — please open an issue. For problems with
dictation in a particular app, say which app (and version), what you expected, and what
the pill showed.

You may modify and redistribute Aural for permitted noncommercial purposes under
the [PolyForm Noncommercial License 1.0.0](LICENSE.md). Commercial use requires
separate permission from Aurathex. See [licensing](docs/LICENSING.md).

Pull requests are not being accepted yet; modification and redistribution rights
do not depend on upstream acceptance. Contributor terms for accepting changes
upstream remain a separate decision.

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
