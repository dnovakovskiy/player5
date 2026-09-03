# player5

A TR-inspired drum machine for the DJ booth. It feeds a channel of a
Pioneer/AlphaTheta mixer and locks tempo and phase to the rig over the
network. Pro DJ Link decks are numbered 1–4; this is the fifth device.

Targets: macOS, web (installable PWA) and iOS, all driven by one Rust core.

* `CLAUDE.md` — decisions, conventions, build and test commands.
* `docs/adr/` — architecture decision records.
* `docs/protocols/` — digested protocol notes with sources.

Quick start:

```sh
cargo test --workspace
cargo run -p render -- patterns/four-on-the-floor.json out.wav --fingerprint
```
