# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Tauri 2 desktop app rendering a Next.js static frontend in the system webview. macOS, Windows and Linux are equal-priority targets; one design language serves all three.

## Users

Serious music listeners who use both Spotify and YouTube Music and want one desktop app for listening, searching and organizing music across both, often while working.

## Product Purpose

LUNOR-X is a unified desktop music player: one interface for discovery, search, library organization, mixed-provider playlists, provider-aware playback, queue management, accounts and cross-device playlist sync. Product philosophy from `PROJECT_BLUEPRINT.md`: "One Application. One Library. Multiple Providers. Full User Control."

Success means the app feels like one coherent desktop product, not two websites embedded in a window, and listeners keep a single library that outlives any one provider.

## Positioning

- **One library, many providers.** Playlists mix Spotify and YouTube Music tracks in one place, and each entry always shows which provider it comes from.
- **The user owns the data.** Open source (MIT), local-first playlists that survive offline, and a backend the user can self-host for sync.
- **Native desktop feel.** It must behave like a single, fluent desktop application on every OS.

## Operating Context

- Long listening sessions on a desktop, usually alongside other work; keyboard use is expected.
- Providers are connected per account; each has its own login method, capabilities and limits (for example Spotify search only in developer mode, YouTube search quota).
- Playlists are edited locally and synced to the LUNOR-X backend; offline edits are kept and conflicts are resolved by the user (keep mine / keep server).

## Capabilities and Constraints

- Required views (blueprint §4.2): Home, Search, Library, Playlist Editor, Player, Provider Connections, Settings.
- Architecture: embedded Rust core in the Tauri process for search, queue and playback state; a separate Rust HTTP API (PostgreSQL) for accounts and playlists. The backend never handles provider credentials, search, playback or streaming.
- The app must never become an unauthorized audio redistribution service. Playback respects each provider's capabilities, licensing, authentication and developer policies; unsupported actions are explained, not faked.
- Current state (2026-10-21): pre-release. Local desktop builds support native YouTube audio, direct YouTube Music search and Google-session-cookie sign-in. Sample library/queue controls remain labeled demos; native controls report actual player state. Release limits: `docs/development/youtube-native-playback.md`.
- Provider branding rules apply wherever Spotify or YouTube marks are shown (`docs/provider-compliance/`).
- Undecided: release channels, Linux token store, production backend hosting.

## Brand Commitments

- Name: LUNOR-X (product and repo); the design system is named LUNOR.
- Voice in shipped copy: plain, calm, specific; names the provider; says what data is kept ("No connection to the server. Your local changes are kept."); no blame, no exclamation marks.
- A design system already exists at `docs/design-system/` (mirrors the LUNOR Design System artifact, https://claude.ai/artifact/TKgE6sy1xhpyMT3tJSSiYd). No logo file exists yet.

## Evidence on Hand

- Product spec: `PROJECT_BLUEPRINT.md`; status: `docs/status/`; provider compliance notes: `docs/provider-compliance/spotify.md`, `docs/provider-compliance/youtube.md`.
- No users, testimonials, metrics, press or release builds exist. Do not fabricate them.

## Product Principles

1. One library first: every surface treats Spotify and YouTube Music items as one collection, with provider origin always visible.
2. Honest capability: show what each provider can and cannot do right now, including demo and unavailable states, instead of hiding gaps.
3. The user's data is safe and theirs: local changes are never lost, sync is explicit, and self-hosting is a first-class path.
4. Desktop-native fluency: fast, keyboard-friendly and consistent across macOS, Windows and Linux.

## Accessibility & Inclusion

WCAG 2.2 AA: text contrast, full keyboard operation, screen reader support, and respect for reduced motion and reduced transparency preferences, on all three operating systems.
