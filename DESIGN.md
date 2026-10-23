---
name: LUNOR
description: Dark, artwork-first design system for the LUNOR-X desktop music player, built on macOS Music conventions.
colors:
  night-canvas: "#000000"
  sidebar-night: "#060607"
  surface: "#101012"
  elevated: "#1d1d20"
  elevated-deep: "#232327"
  hairline: "rgba(255, 255, 255, 0.06)"
  hairline-strong: "rgba(255, 255, 255, 0.10)"
  ink-bright: "#ffffff"
  primary-text: "#f5f5f7"
  secondary-text: "#a3a8b2"
  tertiary-text: "#9ca1a9"
  disabled-text: "#4a5059"
  on-ink: "#000000"
  lumen-lavender: "#c5b4ff"
  lumen-wash: "rgba(197, 180, 255, 0.14)"
  lumen-edge: "rgba(197, 180, 255, 0.40)"
  on-lumen: "#15112b"
  synced-blue: "#8db8f2"
  caution-amber: "#e9b872"
  critical-coral: "#f28b6e"
  frost-100: "rgba(255, 255, 255, 0.05)"
  frost-200: "rgba(255, 255, 255, 0.08)"
  frost-300: "rgba(255, 255, 255, 0.13)"
  frost-dark: "rgba(0, 0, 0, 0.55)"
  scrim: "rgba(0, 0, 0, 0.64)"
  ambient-slate: "#1c2a3a"
  ambient-violet: "#241f36"
typography:
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", Inter, system-ui, sans-serif"
    fontSize: "34px"
    fontWeight: 700
    lineHeight: "40px"
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", Inter, system-ui, sans-serif"
    fontSize: "40px"
    fontWeight: 700
    lineHeight: "44px"
    letterSpacing: "-0.025em"
  title-large:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", Inter, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 600
    lineHeight: "34px"
    letterSpacing: "-0.02em"
  title-medium:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", Inter, system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 600
    lineHeight: "28px"
    letterSpacing: "-0.015em"
  lyric:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", Inter, system-ui, sans-serif"
    fontSize: "32px"
    fontWeight: 700
    lineHeight: "40px"
    letterSpacing: "-0.02em"
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: "18px"
    letterSpacing: "-0.005em"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "18px"
  body-small:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "18px"
  caption:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: "16px"
  caption-small:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: "14px"
  time:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"SF Pro Text\", Inter, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: "16px"
    fontFeature: "\"tnum\" 1"
rounded:
  xs: "6px"
  sm: "10px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  full: "9999px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "6": "24px"
  "8": "32px"
  "12": "48px"
  "16": "64px"
components:
  button-primary:
    backgroundColor: "{colors.primary-text}"
    textColor: "{colors.on-ink}"
    typography: "{typography.title}"
    rounded: "{rounded.sm}"
    padding: "0 16px"
    height: "36px"
  button-primary-hover:
    backgroundColor: "{colors.ink-bright}"
    textColor: "{colors.on-ink}"
  button-secondary:
    backgroundColor: "{colors.frost-200}"
    textColor: "{colors.primary-text}"
    rounded: "{rounded.sm}"
    padding: "0 16px"
    height: "36px"
  button-secondary-hover:
    backgroundColor: "{colors.frost-300}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    rounded: "{rounded.sm}"
    padding: "0 16px"
    height: "36px"
  button-ghost-hover:
    backgroundColor: "{colors.frost-100}"
    textColor: "{colors.primary-text}"
  button-large:
    rounded: "{rounded.md}"
    padding: "0 24px"
    height: "44px"
  play-button:
    backgroundColor: "{colors.primary-text}"
    textColor: "{colors.on-ink}"
    rounded: "{rounded.full}"
    size: "48px"
  icon-button:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    rounded: "{rounded.sm}"
    size: "32px"
  icon-button-active:
    textColor: "{colors.lumen-lavender}"
  search-field:
    backgroundColor: "{colors.frost-100}"
    textColor: "{colors.primary-text}"
    typography: "{typography.body}"
    rounded: "{rounded.full}"
    padding: "0 14px"
    height: "36px"
  segmented:
    backgroundColor: "{colors.frost-200}"
    rounded: "9px"
    height: "32px"
  segmented-selected:
    backgroundColor: "{colors.frost-300}"
    textColor: "{colors.primary-text}"
  nav-item:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    rounded: "8px"
    padding: "0 10px"
    height: "40px"
  nav-item-active:
    backgroundColor: "{colors.frost-200}"
    textColor: "{colors.primary-text}"
  library-row:
    backgroundColor: "transparent"
    rounded: "8px"
    padding: "0 10px"
    height: "48px"
  media-card:
    backgroundColor: "transparent"
    rounded: "8px"
    padding: "0"
  track-row:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    rounded: "{rounded.sm}"
    padding: "0 12px"
    height: "56px"
  track-row-selected:
    backgroundColor: "{colors.frost-200}"
  status-note:
    backgroundColor: "{colors.frost-100}"
    textColor: "{colors.secondary-text}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
  player-bar:
    backgroundColor: "{colors.frost-dark}"
    height: "88px"
    padding: "0 16px"
---

# Design System: LUNOR

## Overview

**Creative North Star: "A Music App That Feels Native"**

LUNOR follows macOS Music conventions in a dark theme: a translucent sidebar, a quiet toolbar, large artwork, sentence-case type and almost no containers. Artwork is the only saturated colour on screen. One small light, Lumen Lavender, marks selection, active controls and links.

Pages are flat. Content sits on the canvas, grouped by space and by large section titles rather than by boxes. Glass is limited to the sidebar, the player bar and the Home and details cards (Frost 100 over a 1px inner rim). The system is one dark theme on macOS, Windows and Linux alike; the overlay title bar is macOS only.

Confirmed rejections: the dot-matrix texture and the Doto display face; bordered cards and panels; uppercase tracked mono labels; repeated "demo" chips; one repeated placeholder artwork.

**Key Characteristics:**
- Artwork leads. Placeholder art is generated from the album id, one composition per album.
- Surfaces are background layers, not outlined boxes.
- One accent with one meaning: what is selected or active.
- Honest states: demo, offline, unavailable and conflict are always shown in words, once, quietly.

## Colors

A near-black palette with translucent white fills, one lavender light and three quiet status tones.

### Primary
- **Lumen Lavender** (`lumen-lavender`): the single accent. Selected nav icon, active toggles (shuffle, repeat, lyrics, queue), the liked heart, "See All" links, the profile button when open. Never a large fill, never on titles.
- **Lumen Wash** (`lumen-wash`): the selected-row tint (sidebar, queue, palette, lists). **On Lumen** (`on-lumen`) is text on a rare solid Lumen fill.

### Secondary
- **Synced Blue** (`synced-blue`), **Caution Amber** (`caution-amber`), **Critical Coral** (`critical-coral`): status icons only, always with words.

### Neutral
- **Night Canvas** (`night-canvas`): the app canvas.
- **Sidebar Night**, **Surface**, **Elevated**, **Elevated Deep**: solid steps used as reduced-transparency fallbacks and for popovers (queue, add-to-playlist, command palette).
- **Frost 100 / 200 / 300**: translucent white fills for hover, fields, segmented controls and inset groups.
- **Frost Dark** and the sidebar and player glass tints: translucent chrome, only on the sidebar and player bar.
- **Scrim**: darkening layer over the Now Playing field and under dialogs.
- **Primary Text** (off-white), **Secondary Text**, **Tertiary Text** (placeholders only), **Disabled Text**, **Ink Bright** (active lyric line).
- **Hairline** (`hairline`): the only line colour. Used under the first section title, between rows of tables and inset groups, and at the top edge of the player.

### Named Rules
**The One Light Rule.** Lumen Lavender means selected or active, and covers under 5% of any screen. Titles stay Primary Text.

**The Clamped Glow Rule.** The Now Playing background is the artwork, blurred, under a 66% scrim, plus a glow clamped to OKLCH L <= 0.25 and C <= 0.10. That keeps text at 4.5:1 or better.

## Typography

**Font:** SF Pro via `-apple-system` (Inter fallback on Windows and Linux). No monospace face anywhere; times use tabular numerals on the system font.

### Hierarchy
- **Page title** (700, 34px/40px, -0.022em): one per screen, left aligned ("Listen Now", "Library").
- **Section title** (600, 22px/28px, -0.015em).
- **Feature title** (700, 20px/24px) over Top Picks tiles.
- **Lyric** (700, 32px/40px, 40px/48px above 1536px wide).
- **Title** (600, 13px/18px): tile titles, track titles, player title.
- **Body** (400, 13px/18px): default text, nav, subtitles in Secondary Text.
- **Group title** (600, 13px, Secondary Text): headers above inset groups.
- **Sidebar group** (600, 11px, Secondary Text, sentence case).
- **Time and captions** (400, 11px, tabular numerals).

Tracking is tight on titles only. Nothing is uppercase or letter-spaced.

## Layout

A desktop shell: a 240px translucent sidebar (72px collapsed) running the full height above an 88px player bar, with a 56px toolbar over the content column. The overlay title bar puts macOS traffic lights over the sidebar's top-left corner, so the sidebar keeps a 28px safe area (56px row) there and Now Playing a 52px row; empty toolbar and sidebar-top areas carry `data-tauri-drag-region`.

The toolbar has back and forward chevrons at left, a centered 36px Search pill (opens the command palette, Cmd+K) and a profile button at right. The sidebar has no search field (the toolbar pill and the Search view own search). Top to bottom: a 56px header (brand mark, "LUNOR-X" in sentence case, collapse button), the primary nav (Listen Now, Search, Explore), the Library panel, and a footer with sync status, Connections and Settings icon buttons and the one quiet "Demo content · no audio" indicator.

Content has 32px side padding, caps at 1680px and centres. Pages open with a Page title and, where content is sample data, one quiet caption under it. Home is "Listen Now": one Featured album (280px cover, 34/40 title, one off-white Play button, a quiet "View album" link), Continue listening (56px track rows in two columns), Made for you (a row of playlist covers) and Songs on repeat (song table). Sync and provider status live in Sync and Connections, not on Home. The queue opens only from the player bar. Grids elsewhere auto-fill from 160px with 20px gutters.

**The Space-Not-Boxes Rule.** Separate groups with space and section titles. A hairline appears only under the first section title, between table rows and between inset-group rows.

Tokens (Tailwind v4 `@theme` in `desktop/src/app/globals.css`): sidebar 240px, collapsed 72px, toolbar 56px, player 88px, content max 1680px, easing `cubic-bezier(0.16, 1, 0.3, 1)`. Every surface stays inside the CSP: no inline `style` attributes or `<style>` elements in the static HTML. Scrubber fills and knobs use script-set styles after mount.

## Elevation & Depth

Layers are Night Canvas, sidebar glass and player glass. Glass is `backdrop-filter: blur(40px) saturate(1.5-1.8)` over a translucent fill (applied through `--lx-glass` and an `@supports` block so the minifier keeps the unprefixed property). Sidebar and player have no rim (the player has one hairline top edge); Home and details cards (`.glass-card`) use Frost 100 with an inset 1px rim. Tiles carry a faint 1px-offset shadow; popovers (queue, add-to-playlist, palette) are solid Elevated with a soft large shadow.

**The Two-Glass Rule.** Glass is the sidebar, the player bar and Home and details cards, with at most two frost levels in view (Frost 100 card, Frost 200 hover). Never put glass on artwork.

**The Solid Fallback Rule.** Under `prefers-reduced-transparency: reduce`, all glass becomes solid Surface with no blur.

## Shapes

8px for nav rows, tile artwork, buttons and filter fields; 6px for thumbnails and the player artwork; 10px for inset groups and status notes; 16px for Top Picks tiles and Now Playing artwork; 12px for popovers; full pills for the Search pill and the Search field; circles for artist art, the profile button and hover play buttons. Line icons use one stroke width, 1.75; transport and liked-heart glyphs are filled.

## Motion

**The One-Feel Rule.** Motion is quiet, short and from one family: exponential ease-out `cubic-bezier(0.16, 1, 0.3, 1)` (`--ease-lumen`), nothing bounces, nothing loops for decoration. Tokens live in `desktop/src/lib/motion.ts` (Motion components) and under `/* motion */` in `desktop/src/app/globals.css` (CSS); the numbers are the same in both.

| Token | Value | Use |
|---|---|---|
| fast | 150ms | hover colour, icon buttons, knob opacity, focus ring is 100ms |
| base | 220ms | view switch (incoming), sidebar width, pill slide, list rise, crossfades |
| slow | 320ms | Now Playing open, native dialog, hero and artwork fade-in |
| exit | 120ms | anything leaving: view out, popover out, message out |
| press | 100ms | button press, scale 0.97 |
| sheet spring | stiffness 320, damping 34 | Now Playing only; damping ratio about 0.95, overshoot far under 2% |

**What animates.**
- *Navigation:* the incoming view fades and rises 8px (220ms); the old one fades out in 120ms; scroll resets to top when the old view has gone. The active row in the sidebar nav, the library rows, the filter chips and the Library tabs share one highlight pill that slides between items (`layoutId`).
- *Lists:* the first 12 children of tile rails, grids, track tables and lists rise 6px and fade in, 30ms apart, once when the list mounts. Item 13 onward appears at once. Removed recent searches collapse (height and opacity). Short sidebar lists animate layout on reorder; lists over 50 items never do.
- *Controls:* buttons press to 0.97; hover colour fades in 150ms; the hover play button on tiles scales in from the cover's lower left corner; the like heart pops 1 to 1.2 to 1 in 180ms when it turns on; the focus ring grows in over 100ms and is visible from the first frame.
- *Player:* bar artwork cross-fades on track change; the progress fill eases linearly (250ms) between ticks but has no transition at all while the pointer is held, so seek and volume follow the pointer exactly; Now Playing rises from the bottom edge on the sheet spring and leaves in 220ms; its ambient artwork layer cross-fades over 400ms on track change; the active lyric line is bright and the others fall off by distance (300ms).
- *Overlays:* the command palette scales 0.98 to 1 and fades (160ms); the queue popover and Add to playlist scale from their anchor corner (180ms).
- *Feedback:* status notes and alerts rise 4px and fade in (180ms); a success tick draws its stroke (240ms); the search spinner turns at 700ms.
- *Sidebar:* width 220ms; labels fade in after a 90ms delay when it expands; the collapsed-mode tooltip fades in 120ms after a 300ms delay.

**What never animates.**
- Anything that implies audio while none plays. The equaliser runs only for a real `playing` state (or the demo "playing" state); it is not drawn when paused and is frozen while buffering. Cover art does not pulse, bounce or spin.
- Lavender. It means live state only; no motion adds or moves it for decoration.
- Looping decoration. The only loops are the equaliser while playing, the skeleton shimmer (1.4s) and the spinner while work is pending. The native Now Playing ambient drift (24s) runs only while playing and is the single slow exception.
- Layout properties on scroll or drag: only transform and opacity animate, except the progress fill width, the shimmer and the stroke draw. Panels never animate blur or backdrop-filter.
- The Scrubber while dragged, hover states that delay visibility, and exits longer than 120ms.

**Reduced motion.** `MotionConfig reducedMotion="user"` removes every transform from Motion components (opacity still fades), and initial states carry no offset. In CSS, `prefers-reduced-motion: reduce` sets transition durations to 0.01ms and turns off stagger, rises, popovers, dialog entrance, shimmer, spinner and tick drawing (the tick shows complete, the equaliser shows static bars, the spinner is replaced by the text beside it). State changes still happen at once, so feedback stays legible. Under `prefers-reduced-transparency` the ambient layers are solid as before.

## Components

### Buttons
- **Primary:** off-white fill, On Ink label, 32px tall, 8px radius. One per view.
- **Secondary:** Frost 200 fill, hover Frost 300.
- **Link:** Secondary Text, Primary Text on hover, with a Frost 100 hover fill; 32px tall. Ghost buttons are never lavender.
- **Icon buttons:** 32px, Secondary Text, Primary Text on hover. Active: Lumen Lavender, no dot, no container.
- **Transport:** filled glyphs without a disc; play and pause are the largest.
- **Focus:** 2px Primary Text ring with 2px offset.

### Sidebar and navigation
The sidebar is one `<nav aria-label="Primary">` on the glass surface, 240px (72px collapsed, width animates 220ms on the Lumen ease, instant under reduced motion; the collapsed flag persists per viewer in localStorage). No borders or dividers between groups; space and a 12px semibold Secondary Text group title ("Library") separate them.

- **Primary nav:** three 40px rows, 8px radius, 20px icons at the 1.75 stroke, 14px medium labels. Active: a filled variant of the glyph (soft fill under the stroke), Frost 200 row, Primary Text, `aria-current="page"`; never lavender, no dot. Hover Frost 100.
- **Library panel:** heading "Library" with a "Show all" link (opens the Library view on the matching tab) and a 32px "New playlist" icon button (opens the Playlist Editor and focuses its name field). Under it, filter chips (All, Playlists, Albums, Artists) as a single-select radiogroup: 28px pills inside 32px targets, selected = Primary Text on Frost 200, arrow keys move the choice. Then a scrollable list (edges fade through a CSS mask only while more content exists) of 48px rows: 40px art (artists round, Liked Songs pinned first with a white heart over generated art), title 13px semibold, subtitle 12px Secondary Text ("Playlist · N songs", "Album · Artist", "Artist"), ellipsis plus `title`. Real local playlists come first after Liked Songs; sample albums and artists follow and are covered by the demo indicator. The now-playing album shows a Lumen Lavender equaliser and "Playing" in its accessible name. States: skeleton rows while loading, an empty line with a secondary "Create your first playlist" button, and "Could not load playlists" with Retry.
- **Footer:** a status dot with plain words from the sync status (Synced, Syncing, Offline, Signed out, Conflict, Sync paused, Sync problem, Status unavailable) and a secondary text action where one helps (Retry, Sign in, Resolve); 36px Connections and Settings icon buttons; then the single demo line. The status is re-read on window focus and view change, not polled.
- **Collapsed:** icon-only nav, 44px covers in 48px rows with no chips or text, the + button, and a footer column of status dot, demo dot, Connections and Settings. Titles appear in a solid Elevated tooltip (title and subtitle) on hover and focus; every control keeps an `aria-label`.
- **Keyboard:** Tab enters the nav; the library list is one tab stop with roving focus (Up, Down, Home, End); Enter opens, Space or P plays (demo), Alt+Up and Alt+Down reorder playlists (order stored per viewer in localStorage). Cmd/Ctrl+B toggles collapse. Drag-and-drop reorder is not implemented.

### Settings category rail and pane header
Settings is split into six categories: General, Playback & audio, Accounts & providers, Sync, Keyboard shortcuts, About & diagnostics. From 1100px a 200px rail sits inside the page (not the app sidebar), with a 36px "Search settings" field above it; under 1100px the rail becomes a wrapping row of tabs under the field. Each tab is 36px tall: an 18px icon at the 1.75 stroke and a sentence-case 13px medium label. Selected: Primary Text on Frost 200 through the shared ActivePill (`settings-cat-pill`), never lavender; others Secondary Text with a Frost 100 hover.

- **Pane header:** one h2 (22px semibold, display face) and a one-line 13px Secondary Text description. Group titles inside the pane are h3 (Inset reads `InsetLevel`), 13px semibold Secondary Text, over inset groups with hairline rows. No nested cards.
- **Keyboard:** the rail is a `tablist` (`aria-orientation` follows the layout) with roving tabindex and automatic activation. Up/Down in the rail, Left/Right in the row, Home and End. The pane is the single `tabpanel`, labelled by the selected tab. Focus ring: 2px Primary Text, 2px offset.
- **Persistence:** the category lives in local state, remembered per viewer in localStorage (`lunorx.settings.category`); `SettingsScreen` takes an optional `initialCategory` for links from other screens.
- **Search:** filters categories and rows by label over a static index; choosing a result switches category and focuses that group's heading. No match shows "No settings match". Escape clears.
- **Platform groups:** the "Playback menu (macOS)" shortcuts are shown only when the user agent is macOS and say so.

### Segmented control
A Frost 200 recessed track with the selected segment raised on Frost 300. The Library tabs use a 36px track with 32px segments (roving tabindex; Left, Right, Home and End switch tabs).

### Library, Playlist Editor and Queue
- **Library:** tabs, a 220px filter, a sort select (Recently added, Name, Artist) and a grid/list toggle saved per viewer in localStorage. Grid tiles are `auto-fill, minmax(176px, 1fr)`; list rows are 56px. Playlist tiles say `N entries · Not uploaded | N changes waiting | Partly uploaded, X of Y | Synced`. Empty tabs name the next action; sample tabs carry one demo caption.
- **Playlist Editor:** under 1100px one column (list, then details with an All playlists back button); from 1100px a 320px list and a details pane. The create form validates inline (Enter submits, error is role=alert, focus returns to the field). Details show a conflict banner with Keep mine and Keep server (Keep server asks once to confirm), plus upload, resume and send-waiting-changes buttons. Every action writes one polite status line or one alert with fixed copy. The track list is shown as unavailable because no IPC command returns entries.
- **Queue:** popover section list, Now playing then Up next, 56px rows with a thumbnail play button. No remove or clear buttons because the store has none.

### Tiles and rails
Square artwork at 8px radius, title (13px semibold) and subtitle (13px Secondary Text) underneath, no container. A small dark circular play button appears over the art on hover and keyboard focus. Rails scroll horizontally, snap to tiles and hide their scrollbar. Top Picks tiles are full-bleed artwork at 16px with a bottom scrim tinted from the art's tone, title and subtitle laid over it.

### Featured album
One per Home. A glass card holds a 280px cover (10px radius, separate focus target that opens details), a 34/40 bold title (also a separate button; two-line clamp, `title` attribute), artist at 17px, a year and provider line, then one 44px off-white Play button and a quiet "View album" ghost button. Play is always visible.

### Track row and now-playing row
56px row, 40px thumbnail (art seeded from album and track id), title, artist, provider in plain sentence case, duration. The thumbnail carries the play button, shown on hover and `:focus-within`; its label reads "Play <title> by <artist>, demo, no audio". The row is not interactive itself. The current track gets Lumen Wash and, while "playing", a three-bar Lumen Lavender equaliser (`.lx-eq`) in place of the glyph.

### Song table
Glass card, sentence-case gray header (#, Title, Album, Provider, Time), hairlines between rows, `table-layout: fixed` with single-line ellipsis and `title` attributes. The # cell becomes the play button on hover and focus. Album and Provider columns drop out below 1024px and 768px.

### Details view
Album or playlist: Back button, 240px cover, title as the page h1 (34/40, two-line clamp), artist or blurb, a meta line (kind, year, count, minutes, provider), one Play button plus 44px Shuffle and Add to library icon buttons, then a glass card song table. A playlist with no songs shows an explanatory empty state. Sample data, labelled demo.

### Mood tile
Explore only. 96px-tall, 12px radius, generated art with the tone scrim, label inside at 20px bold, one button per tile (4 columns from 1024px, 2 below). Opens the matching sample playlist.

### Top result row
Search only. One 96px row in a glass card under a "Top result" heading: 72px thumbnail, title at 22px semibold, artist and provider line, Add to playlist and Open on YouTube icon buttons (32px). The same hit is not repeated in the provider list below.

### Lists and tables
Rows are 56px with a 40px thumbnail. Tables (Songs) have a sentence-case semibold gray header row and hairlines between rows; plain lists have none. Selected or hovered rows use Frost 100 or Lumen Wash.

### Inset groups
Apple Settings style: a Frost 100 rounded-10px group, rows divided by hairlines, a gray 13px semibold title above. Used for Settings, Sync, Accounts and Connections, where forms and key-value rows need grouping. Never nested.

### Setting rows and status rows
Inside an inset group, a setting row is 44px minimum: label (13px Primary Text) and an optional 12px Secondary Text hint at left, value or control at right; `stack` puts a wide control under the label. A status row is a setting row whose value is a 16px line icon plus words (Supported, Not confirmed, Unsupported, Conflict); the tone colours the icon only. Notes (errors, warnings, results) are full-width rows with the same icon and words, `role="alert"` for errors. Shared in `features/settings/rows.tsx` for Settings, Connections, Accounts, Sync and Player. Destructive actions use a two-step confirm row inside the group, never a modal.

### Settings: Lyrics group
In Playback & audio, between Disk cache and What can play, an inset group "Lyrics" (`settings-lyrics`). Its first row is "Auto order" (hint: Automatic prefers word-timed lyrics, then timed, then plain text; ties go to the source higher in the list; My LRC always comes first), then a reorderable list, one row per source core reports (`lyrics_settings_get`): the label at left; at right the 28x16 off-white switch "Use this source" (accessible name "Use this source: <label>") and two 32px icon buttons "Move <label> up" / "Move <label> down" (`aria-disabled` at the ends so focus stays). Alt+Up and Alt+Down on a row move it too. Changes show at once and are saved with `lyrics_settings_set`; a failed save puts the list back and shows a `role="alert"` row "Could not save the change. The list is back as it was." with Retry; a failed read shows "Could not read the lyrics sources." with Retry. A polite live region announces each move. The last row is the honest privacy note: "Other sources are unofficial services. Turning one off stops sending the song title and artist to it." The old per-viewer preference `lunorx.lyricsSource` (`auto` or a source id) stays and is set only from the Now Playing source sheet. Search index entries: "Auto order", "Lyrics sources", "Use this source".

### Inputs
The toolbar Search pill and the Search view field are Frost 200 pills; other fields are 8px, Tertiary Text placeholders, lifting to Frost 300 on focus. Sort is a native select styled as a field.

### Scrubber
A transparent native range over a 3px track; the fill is Secondary Text and the 12px knob appears on hover and focus. Times sit at both ends in 11px tabular numerals. Volume uses the same control, thinner and shorter.

### Player bar
88px, translucent, one hairline top edge. Left: 56px art at 6px radius (opens Now Playing), title and artist in 13px, heart. Centre: transport, then the scrubber with times. Right: lyrics, queue, AirPlay (disabled, no device), volume, expand. Titles are Primary Text.

### Now Playing and Lyrics
Lyrics closed: artwork centred at 16px radius with title and artist under it, the close chevron top-left right of the traffic lights (76px inset, 24px in native full screen), the controls pill at the bottom. The background is the blurred artwork under a scrim (`--np-scrim`, default 0.65 black; the worst case, a white cover, still gives 14.7:1 for the active line and 5.5:1 for neighbours).

Lyrics open: two layouts, a 2-icon radiogroup in the pill ("Lyrics layout": "Full window lyrics", "Artwork and lyrics"), remembered in `lunorx.lyricsLayout` (`full` default, `split`).
- **Full window.** No panel and no container: lyrics sit directly on the ambient background and fill the window. Top-left cluster: close chevron, 52px artwork at 8px radius, title (17px semibold) with the heart, artist (14px Secondary Text) with the equaliser only while really playing (heart and equaliser centred on the title and artist lines), then one notes line (below); about 84px tall. Lyrics are a left-aligned column centred in a 28em measure (capped by the window width minus gutters, 1680px at most; left edge at the chevron inset, 76px, 24px in full screen). Active line anchors at 40% of the window height (a lead spacer lets line 1 reach it). The mask is fixed in pixels: fully faded to 120px from the top (clears the cluster, also with a fallback note), full at 176px; at the bottom full 208px above the edge and fully faded 140px above it, so no line reaches the pill (its top is 124px above the edge, 100px high, 24px from the bottom). While the pill is hidden the fade end moves to 24px above the bottom (200ms, an animated `@property`).
- **Split.** Artwork (max 420px) with title, artist, heart and notes on the left; lyrics on the right, again directly on the ambient, in the same type system at the smaller size. Active line also anchors at 40%.
- **Controls pill** (both layouts, glass, 24px radius, no border, max 760px, bottom centre; Full window floats it, Split and closed state place it in the flow). Row 1: times and seek bar. Row 2: left the source button "Source: <name>" (lyrics open; it opens the Source sheet below and truncates inside its column), centre play/pause, centre play/pause, right the line counter "Line N of M" (13px, tabular; "Intro" before line 1), the "Lyrics options" button, the layout toggle (kept in the row: measured one row at 1100x680, 1280x800 and 1440x900), "Enter full screen" / "Exit full screen" (Maximize2/Minimize2) and the lyrics toggle. Targets are 32px (segmented options, inside a 36px group) to 44px. The layout toggle shows its choice without lavender: the selected icon is Ink Bright on Frost 300 with a heavier stroke, the other is Tertiary Text. The options popover opens upward. The pill glass is `rgba(20,20,28,0.84)` with a 32px blur, so no line reads through it; under reduced transparency it is solid Elevated Deep.
- **Auto-hide (Full window only).** The pill and the close chevron fade out in 200ms after 3s with no pointer move (a pointermove at the same coordinates does not count), pointer down, key, touch or wheel, only while the player state is "playing". They never hide while paused, while the options popover is open (`aria-expanded`), while keyboard focus (`:focus-visible`) is inside them, or under `prefers-reduced-motion` (then they stay). Any input brings them back at once. The cursor is `none` while they are hidden (never under reduced motion). The notes are not part of this and never hide.
- **Notes (honesty labels).** One quiet line under the artist, 12px Secondary Text, always visible, parts joined with a middle dot in this order: source attribution, state, honesty notes. Timed: "<source label> · synced"; with real word timings and "Highlight words" on, "<source label> · word by word"; My LRC: "My LRC · made by you". A source credit that says more than the label (for example "Source: LyricFind") follows the label; the credit is also the label's hover text. No note ever claims estimated words: there are none. Estimated: "<source label> · estimated timing" with an info icon after "estimated timing", which is Primary Text; hover text and screen-reader text carry the full sentence plus the tap-to-correct hint. Untimed: "<source label> · not timed". The first letter is upper-cased by CSS. The fallback note (Primary Text, `role="status"`) is the only separate line, and only while present. The estimate hint and "Reset corrections" live in the "Lyrics options" popover, under "Follow along". Nothing the user must know sits in the auto-hiding pill.
- **Full screen.** The pill button and the `F` key (not in a text field, no modifier) call the window's `setFullscreen(!isFullscreen)`. The button appears only after the window API answers once and hides itself if a call throws. Capabilities: `core:window:allow-set-fullscreen`, `core:window:allow-is-fullscreen`. State is re-read after a toggle and on window resize.
- **Gaps.** Loading: six soft skeleton bars where the lines will be, with a screen-reader "Loading lyrics". No lyrics: large artwork (Full window) with "No lyrics found for this track" and "Checked every source that is turned on."; instrumental: "Instrumental · no lyrics"; failed: "Lyrics could not be loaded" and "Playback can continue." (no Retry: the app has no refetch path). The source button stays in the pill in all of them (loading shows "Source: loading" and skeleton rows in the sheet). Unknown duration keeps the plain text view with its note.

### Synced lyrics (native)
Native Now Playing shows lyrics on the ambient background with no panel (see Now Playing and Lyrics). Timed lyrics follow real timestamps; untimed text can follow an estimate, which is always labelled as one (see Estimated follow).
- **Timed** (the chosen source has real timestamps: `synced` and `lines`): Lyric type per line, same size for every line: Full window `clamp(34px, 3.2vw, 60px)` in a 28em measure, Split `clamp(28px, 2.4vw, 44px)` in 22em, 700, line-height 1.25, letter-spacing -0.02em, `text-wrap: pretty`, 0.1em item padding. The active line is Ink Bright with a 2px lift on the Lumen ease (no spring); the others are Primary Text, faded by distance from the active line, done in CSS on the line (not the item) so only the active row re-renders: neighbours 55%, two away 38%, three 30%, four 22%, further 16%, plus a blur of 1px at three and four away and 2px beyond (none in the intro, and none under reduced motion or reduced transparency). Measured on 40 mock lines at 1280x800: 7 lines at 20% or more after the mask with mostly 36-character lines (6 when every third line has 100 characters), 8 at 1440x900 (6 with the long lines). Opacity, filter and transform change in 300ms on the Lumen ease. A line becomes active 150ms before its timestamp. Words inside the active line are highlighted only when the source delivers real word timings (see Word highlight). The first 8 lines rise in once per lyrics view (a new track or source), 30ms apart. The active and neighbour lines hold 4.5:1 on the brightest scrim case; the 38% lines and further are decoration.
- **Instrumental gap (timed lyrics only):** when the next line starts more than 6s after the active one, or the first line starts after 3s, a row of three Primary Text dots (0.26em) opens above that next line. It stays hidden for 2.5s after the active line starts (and shows from 0 before line 1), then opens in 300ms and fills as the real clock approaches the next line (progress in twelve steps, a dot is 25% to 100%; each step fades in 300ms), breathes (scale 0.82 to 1.12, 2.4s loop, 160ms apart per dot, only while live) and fades out when the line starts. The row stays open as a stanza break; past rows open instantly, never animate. `aria-hidden`, no announcement. Never shown for estimated lyrics. Reduced motion: no height transition and no breathing, the dots stay static and filled by progress.
- **Chrome:** the line counter and the "Lyrics options" button (popover with "Follow along" in estimated mode, and "Highlight words" only when the chosen source has real word timings) live in the controls pill; the source button sits at its left. Hints and notes live under the artist line (see Notes). The seek bar and times stay as they are.
- **Auto-follow:** the active line is kept at about 40% of the panel height with smooth scrolling (instant under reduced motion). Scrolls are coalesced: a smooth scroll starts at most once per 260ms and always aims at the latest target (measured at 1 per 250ms on lines 200ms apart), and a target more than 1.5 viewports away jumps instantly. A wheel, touch or scroll key pauses following for 4 seconds; while paused and the active line is off screen, a 36px secondary button "Back to current line" appears. Clicking it, or waiting, resumes.
- **Click to seek:** every line is a button (`aria-label` "Jump to <line>") that seeks to its timestamp through the existing seek command. The active line has `aria-current="true"`; the lyrics are not a live region.
- **Clock:** Core reports progress about once a second. Between reports the position advances with `performance.now()` only while the player state is "playing", and holds when paused, buffering or idle. Every report and seek re-syncs it. A frame loop runs only while timed lyrics are on screen, the player is playing and the document is visible. Reduced motion does not stop the clock; it removes scale, line entrance and smooth scrolling.
- **Untimed** (YouTube Music text, or an LRCLIB record without timestamps): plain text, no active line, a gentle fade-in, and the note "Not timed: the current line is unavailable." under the source line.
- **Estimated follow** (untimed text only, never on timed lyrics): when the track duration is known and "Follow along" (accessible name "Follow along (estimated)") is on (default, remembered in `lunorx.lyricsEstimate`), the text is spread over the song (blank lines are stanza breaks) and followed with the same look, scroll and clock as timed lyrics. It is always labelled: "estimated timing" with an info icon in the notes line (hover text: "This song has no timestamps. The highlight follows an estimate and can be a few seconds off."), the counter reads "Line N of M (estimated)", and the source line stays. A line is a button that does not seek: `aria-label` "Mark "<line>" as the current line"; tapping it pins that line to the current position and re-spreads the lines around it. The hint "Tap the line being sung to correct the highlight." shows in the "Lyrics options" popover (and in the flag's hover text) until the first correction in the session; a secondary "Reset corrections" button replaces it once a correction exists. Corrections are kept per track and per source (`lunorx.lyricAnchors.<trackId>:<sourceId>`, last 30; keys without a source suffix are ignored). The estimate cannot know instrumental breaks, repeated choruses or pauses, so it drifts there until corrected. The switch is a 28x16 off-white pill (`role="switch"`, not lavender); off, or with no duration or an instrumental, it returns to the plain text and the note below.
- **Word highlight (real word timings only):** a line shows words only when the chosen source delivers them (`line.words` non-null, `source.words`). There is no estimate: lines and sources without word timings light as a whole line, as before. States on the active line: sung words Primary Text; the current word Ink Bright with a small lift (`translateY(-1px) scale(1.03)`, origin bottom, 160ms ease-out); upcoming words 55% opacity (4.5:1 on the darkest and the brightest scrim). Colour and opacity change in 200ms on the Lumen ease. No gradient text, no `background-clip`. Reduced motion drops the lift and scale and keeps the colour step. Text between words comes from the line itself, so spaces and punctuation render as given. The word shown is the one under the same clock as the active line, 150ms ahead; after the last word all words read as sung. It is computed in the existing single frame loop and kept in state that changes only when the clock crosses a boundary, so only the active row re-renders. The switch "Highlight words" (`role="switch"`, same 28x16 pill, remembered in `lunorx.lyricsWords`, default on) appears in the options popover only when the chosen source has word timings.
- **Source sheet:** the pill carries one button, "Source: <name>" with a chevron (Auto reads "Source: Auto · <label shown>"; the label truncates). It opens a solid Elevated Deep sheet upward, 360px wide, 16px radius, shadow, no border, `role="dialog"` named "Lyrics source" (not modal; focus is trapped inside, Tab and Shift+Tab wrap, focus lands on the chosen radio, Escape goes back one level and then closes and returns focus to the button without closing Now Playing, an outside press closes). Top to bottom: title "Lyrics source" (15px semibold) with the quiet line "Just for this song. The default order is in Settings." and a 40px gear button that closes Now Playing and opens Settings > Playback & audio > Lyrics; one scrolling `radiogroup` "Lyrics source"; a fixed Timing row; a fixed "Wrong song? Search by title" row with a chevron. Sentence case, 13-15px, no eyebrow labels.
  - **Rows.** "Automatic" first on Frost 100 (sparkle glyph, sub-line "Showing <label of the source Auto picked>"), then every source in the response in the user's Auto order, My LRC first when it exists. Each row is one radio, min 42px tall (40px or more hit targets). The dot is a 16px ring, filled Primary Text when chosen (never lavender). Right side: usable rows show badges on Frost 300 pills: "Synced" (real line timestamps), "Word" (real word timings, with Synced), "Text" (plain only); a match is "Text only" with the title "Length differs, so only plain text is available" when the length differs; "Matched by you" tags a hand-picked match and an "Unbind" text button follows. Unusable rows are disabled radios at reduced contrast with the reason in `title` and `aria-label`: "Not found", "Failed", "Unreachable", "Turned off", "Instrumental". "Unreachable" and "Failed" add a "Try again" text button (calls `lyrics_source_fetch`, replaces that entry in place, shows a spinner and "Trying" while pending, keeps focus in the sheet, and announces the result in a `role="status"` line: "<label>: lyrics found." or "still unreachable"). Offline: a note "You are offline. Lyrics already loaded still work; Try again and search need a connection." and Try again, Unbind and search are inert. Loading: five skeleton bars and "Loading lyrics sources" for screen readers. Failed info: `role="alert"` "Lyrics sources could not be loaded. Playback can continue."
  - **Keyboard.** Tab and Shift+Tab cycle the sheet. In the radio group: ArrowDown/ArrowRight and ArrowUp/ArrowLeft move focus (wrapping, skipping disabled rows), Home and End jump, Space or Enter choose. A 2px Primary Text focus ring is drawn inside each control. Choosing a source keeps the sheet open (so Timing can be adjusted), keeps the position and re-runs the follow for the new text.
  - **Timing offset.** Shown only when the chosen source has real timestamps (never for plain text or estimated lyrics): "Timing" with the caption "Lyrics ahead of the singer? Move them later.", a pill stepper (minus, value as "0.00 s" / "+0.25 s", plus; 40px buttons) in 0.05 s steps from -5.00 to +5.00 s, press and hold repeats (first repeat after 400ms, then every 70ms), and "Reset" when not zero (focus moves to the plus button). The offset is per track and per source, kept in `lunorx.lyricsOffset.<trackId>:<sourceId>` (removed at zero). It shifts every line and word start and end, so the active line, the word state and the click-to-seek target all use the shifted time (a click seeks to the line's moved start).
  - **Search by title.** The "Wrong song?" row opens an inline view in the same sheet: back chevron, title "Search by title", a 40px field prefilled "<title> <artist>" (Enter searches every enabled source with `lyrics_source_search`), then rows with title, "artist · m:ss · source label", and Synced / Word badges. Choosing a row calls `lyrics_source_bind` (core persists it), replaces that source's entry, selects it and returns to the list, where it carries "Matched by you" and "Unbind" (`lyrics_source_unbind`, then a fresh fetch). States: skeleton rows while searching, "No other matches", a `role="alert"` "Search failed." with Retry, "That match could not be used. Try another." Escape goes back one level.
  - **Auto** (frontend mirror of the core rule): My LRC first when it has timed lines; otherwise the best usable source by real word timing, then line timing, then plain text, with ties going to the user's order (`lyrics_settings_get`). A specific choice wins when that source has lyrics. When it has none, the view falls back to Auto with a visible note under the artist line, never silently: "<X> had no lyrics for this track. Showing <Y>." ("could not be loaded", "is turned off" and "lists this track as instrumental" variants). The stored choice is kept, so it applies again on a track that has lyrics from that source. Ids can be missing or unknown (a newer core): only what arrives is rendered.
  - **Attribution** follows the chosen source in the notes line (see Notes). The choice is remembered in `lunorx.lyricsSource` (`auto` or a source id), shared live across tabs.
- **Edges:** Full window uses the pixel mask described under Now Playing and Lyrics; Split keeps a percentage mask (transparent to 12%, full from 22%, fading from 84%) on a scroll box that ends above the in-flow pill.

### Lyric Maker
A full-window layer inside the Now Playing dialog for making a `.lrc` file while the song plays and no lyrics are at hand. Opened by "Make lyrics file" (pill, `FilePenLine`, and the "Lyrics options" popover); `Escape` closes sheets and popovers first, then the maker, and only then Now Playing (the layer stops the key). It covers the stage (`invisible` and `inert` underneath) with a 78% black scrim over the ambient artwork (solid under reduced transparency). It records only what the user taps or types; nothing is guessed, and no accuracy is claimed (see Calibration). Lavender is not used: nothing in it is a live provider state.
- **Layout.** Header: 52px artwork, title, artist, one 12px line "Delay 238 ms · Consistency 91%" (or "Not calibrated: using 220 ms", or "Delay N ms · set by hand"); at right the save status ("Saved HH:MM:SS", `role="status"`, `aria-live="polite"`; "Saving…" is `aria-hidden`), the file chip (basename, `title` = full path) with "Reveal in Finder" and "Copy path". Centre: the lines directly on the background in the lyric type system (`clamp(26px, 2.5vw, 42px)`, 700, -0.02em) in a 1000px column left-aligned to the 76px inset; each row is a 112px gray tabular `[mm:ss.xx]` column and the text, wrapping; the last recorded line is Ink Bright (`aria-current="true"`), the rest 62%, 90% on hover or focus. Row tools (earlier / later 0.1 s, Re-stamp, Delete) show on hover and focus only. Type along keeps the newest line just above the field. Below: the input row (a 56px Frost 200 field in 24 to 34px Display type, the pending stamp at left, the live song clock at right). Bottom: the pill (760px): seek bar, then Back 5 s, Calibrate, Keys, play/pause, the Type along / Tap along radiogroup, Undo, Redo, Done. Buttons keep the text field focused (their `mousedown` is prevented) so transport works while typing.
- **Type along** (default, remembered in `lunorx.lyricMakerMode`). The field is always focused. The first key that yields text into an empty field (not a modifier, arrow, Tab, Escape or a space), or the first `compositionstart`, or a change event (paste), stamps the line: `start = position at event.timeStamp - delay`. Tab stamps again (focus stays), Shift+Tab moves the pending stamp 200 ms earlier, Enter adds the trimmed line (an empty one does nothing), Cmd/Ctrl+Enter stamps the next line now (up to 3 stamps wait, "2 lines waiting"; text fills the oldest stamp first), backspacing to empty drops an automatic stamp (an explicit one stays). Lines are inserted in time order (equal times keep the order made). Enter during an IME composition (`isComposing`, keyCode 229) never commits.
- **Tap along** (when text exists: the available source text or pasted lyrics). Rows show `[--:--.--]` until stamped; the cursor row is `aria-current="step"`. Space or Enter stamps it (position at the key event minus the delay) and advances past blank lines, Backspace un-stamps the last stamped line and steps back, Up and Down move the cursor, a row button stamps that row. Unstamped lines are never written. Saved lines that match no script line stay in the file and are counted in a hint.
- **Editing.** Click a line to edit it inline (Enter saves, Escape cancels); the -/+ buttons (hold to repeat) and `[` and `]` on a focused row move it 0.1 s; Delete removes it. Undo and redo (Cmd/Ctrl+Z, +Shift) keep the last 50 operations (pure reducer in `lib/lyricMaker.ts`); inside the text field with text in it, Cmd/Ctrl+Z stays the browser's own text undo.
- **Stamping rules.** Allowed only while the player is playing, paused or buffering; otherwise the field stays editable but stamps nothing and says why ("Stamping is off: the player is ready."). The position is `lastReported + (event.timeStamp - reportedAt)` while playing (capped 2 s past the report, the existing lyrics clock) or the held position while paused, minus the delay, clamped to 0 and the duration, stored in ms. Files are written at centisecond precision by core. Back 5 s seeks to position - 5 s.
- **Calibration** (modal sheet, `lunorx.lyricMakerCalibration`: `{ offsetMs, consistency, jitterMs, date, manual }`). The song pauses; 12 beeps (880 Hz, 60 ms, gain ramps), 1.0 to 1.6 s apart from a seeded jitter, through WebAudio; the user presses Space on each. Beep time is mapped to `performance.now()` through `getOutputTimestamp()` (else `baseLatency + outputLatency`). Each tap is matched to a beep (taps up to 100 ms early count; later than 800 ms, doubles and strays are dropped); the first 2 beeps are practice; deltas outside -100 to 800 ms are discarded; the delay is the median of the rest, clamped to -500..500 ms; jitter is 1.4826 x MAD; consistency is the share of the 10 counted beeps whose tap landed within 300 ms of the median (a missed beep counts against it). Fewer than 5 usable taps changes nothing. The result is written in plain words ("Your typical delay: 238 ms (applied automatically)", "Consistency: 91% of your taps land within 300 ms of the typical delay.") with the verdict "Good enough for a reliable file." at 80% or more, else "Taps are inconsistent. Try again, or use Tap along on a calmer song. Slowing the song down is not available yet." The sheet says the beeps use the browser audio path and the song the native player, so the Delay stepper (-500 to +500 ms, 10 ms, hold to repeat; marks the delay "set by hand") covers the remainder. The 80% target is measured for the user, never asserted.
- **Autosave.** Every change that alters the list calls `lrc_save` (sorted stamped lines only; an empty list deletes the file). Saves are serialised: one in flight, the latest list wins, a failure keeps the lines in memory and in the window and shows the message and Retry ("Could not save to Documents. Your lines are kept in this window."). Done after an error needs a second press ("Close without saving"). A failed `lrc_load` pauses saving (the read-only field) until "Try again" or "Start a new file", so an existing file is never overwritten blind. Opening loads an existing file and says "Continue from line N" with a "Go to line" link.
- **My LRC source.** A frontend-added source ("My LRC", never badged "Synced" differently from other timed sources) listed first when `lrc_load` reports a file with lines; absent otherwise. Auto prefers it first; its lines feed the timed follow like any timed source and update at once after each save. The notes line reads "My LRC · made by you" (never "estimated timing"). Keyboard shortcuts gains a "Lyric Maker" group.
- **Motion.** Fades only: 150 ms hover, 180 ms sheet rise (off under reduced motion), smooth scroll of the working line (instant under reduced motion). The song clock runs on one `requestAnimationFrame` only while the player is playing and the page is visible, and writes the DOM directly (no React render per frame); closing the maker stops it.

### Honest state
Demo content is flagged once, in the sidebar foot ("Demo content · no audio"; a small dot alone when collapsed) and in a caption under sample page titles. Now Playing repeats it as one line under the transport because the sidebar is hidden there. Playback controls keep "(demo)" in their accessible names. Real data (playlists, sync, provider status) never carries it.

## Do's and Don'ts

### Do:
- **Do** let artwork be the only saturated colour; generate placeholders from the album id so no two match.
- **Do** build pages from space, large titles and rails; use groups only for forms.
- **Do** keep Lumen Lavender for selection and active state, under 5% of the screen.
- **Do** pair every status tone with an icon and words.
- **Do** honour `prefers-reduced-transparency` and `prefers-reduced-motion`.
- **Do** take durations and the ease from the Motion tokens instead of inventing new ones.

### Don't:
- **Don't** draw borders or cards around panels, nest groups, or box heroes.
- **Don't** use uppercase, letter-spaced or monospace labels, or per-section demo chips.
- **Don't** reintroduce the dot-matrix texture or the Doto face.
- **Don't** colour titles or buttons Lumen Lavender; primary actions are off-white.
- **Don't** use provider brand colours or logos in LUNOR chrome.
- **Don't** put glass anywhere except the sidebar and the player bar.
- **Don't** add a looping or bouncing animation, or any motion that suggests audio is playing when it is not.
- **Don't** use a circle inside a circle as placeholder art.
