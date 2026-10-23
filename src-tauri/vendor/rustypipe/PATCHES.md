# Local patches to rustypipe 0.11.4

Upstream: https://codeberg.org/ThetaDev/rustypipe (GPL-3.0, see LICENSE). This copy is used through `[patch.crates-io]` in `src-tauri/Cargo.toml`.

1. `src/util/visitor_data.rs`, `new_visitor_data`: `fetch_visitor_data().await.unwrap()` became `...await?`.
   Upstream panics when YouTube Music answers the visitor-data request with a non-success status
   (seen: `302 Found`), which killed the calling task. With `?` the caller gets an `Error` and can report it.

2. `src/model/frameset.rs`, `Frameset::urls`: return type `FramesetUrls<'_>`. Only silences a new compiler lint (`mismatched_lifetime_syntaxes`) so the build stays free of warnings.

Drop this directory and the `[patch.crates-io]` block once an upstream release returns the error instead of unwrapping.
