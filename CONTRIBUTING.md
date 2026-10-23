# Contributing to LUNOR-X Desktop

## Setup

Read the [README](README.md) for requirements, layout and build commands. Work in a branch off `main`.

## Quality gates

Run the gates for every part you touch.

Rust core, from `src-tauri/`:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo check
```

Frontend, from the repo root:

```sh
bun run lint
bun run build
```

`lint` is `tsc --noEmit`. There is no frontend unit test script yet.

The live backend test (`src-tauri/tests/backend_sync_live.rs`) runs only when `LUNORX_LIVE_BACKEND_URL` is set. Otherwise it skips.

## Rules that apply to every change

- **Architecture boundaries.** UI → Tauri IPC → Core. Commands stay thin. The frontend does not own search aggregation, queue or playback state.
- **Provider compliance.** Do not add search or playback for a provider until its compliance review is recorded. Do not use Lavalink or unofficial stream extraction, except the YouTube exception documented in `AGENTS.md`.
- **Approval for playback work.** Playback implementation needs an approved plan first.
- **IPC.** Validate every argument. Do not rename commands or DTO fields to satisfy a lint.
- **No secrets.** Do not commit credentials, real `.env` values, or provider tokens.

## Commits

Every commit must be signed and verified. Check with:

```sh
git log --format='%h %G? %s' -5
```

Each line should show `G` in the second column.

## Pull requests

- Keep changes small and focused on one concern.
- State which quality gates you ran and their results.
- Do not change `LICENSE` without a maintainer decision.

## Conduct

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md). Report security issues as described in [SECURITY.md](SECURITY.md).
