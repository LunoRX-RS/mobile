@AGENTS.md

# Claude Code notes

- `AGENTS.md` above is the shared rule set. Keep project rules there, not here, so the two files cannot drift. Add only Claude-specific guidance to this file.
- Before UI work, read `PRODUCT.md` and `DESIGN.md`. Impeccable commands (`/impeccable ...`) read `.impeccable/design.json`; regenerate it with `/impeccable document` when `DESIGN.md` changes.
- Run the checks in `AGENTS.md` for the part you touched (frontend or `src-tauri`) and quote the actual result. If a check cannot run, say so instead of skipping silently.
- The working tree may have unrelated uncommitted changes. Stage only the files your task touched; never `git add -A`.
- Do not commit, push or open a PR unless asked.
