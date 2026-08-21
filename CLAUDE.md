# Antigravity Unlocker — agent rules

Windows tool (Rust + Python) that unblocks Antigravity Desktop/IDE for Google accounts in gated regions: renames a proto field in the Language Server binary and keeps 4 Google hosts resolving through a Russia-geolocated DNS resolver (NRPT + local relay + IP_UNICAST_IF VPN bypass). Crate `ag_unlocker`.

## Commands
| Task | Command | cwd |
|---|---|---|
| build (release) | `python build_rust.py` | repo root |
| test | `cargo test` | repo root |
| test (heavy, real installs) | `cargo test -- --ignored` | repo root |
| keygen | `python dist_keygen.py` | repo root |

## Never
- Do NOT build a release with `cargo build` directly — always `python build_rust.py`. It alone syncs the Cargo version, regenerates the keygen, moves + UPX-packs the exe to `release/AG_<ver>.exe`, writes the `CANARIES.md` ledger row, and runs `cargo clean`.
- Do NOT read, quote, copy, or reference the CONTENTS of `.secrets.json` anywhere. Its existence/purpose may be mentioned; its values must not. Same rule for the values of `LICENSE_BASE_SECRET` / `CANARY_SEED` / `SECRET_PHRASE` (named, never printed).
- Do NOT bump the Cargo.toml 3-digit version casually — it re-salts and rotates ALL existing user license keys (auth.rs + dist_keygen.py must stay in lockstep). The 4th digit is safe.
- Do NOT broaden the NRPT host list beyond the 4 core hosts — it leaks DNS to a third party for zero benefit.
- Do NOT install the DNS relay to `%LOCALAPPDATA%` — a scheduled task there fails with `0x80070002`; it must live in `%ProgramData%`.

## Context
`ROADMAP.md` is the project state — read it once before substantive work. Deep detail lives in `.claude/kb/` (patch, dns, build-and-canary), indexed at the end of ROADMAP.md. Both are maintained by the `roadmap` skill. User-facing docs (`README.md`) are Russian and separate.
