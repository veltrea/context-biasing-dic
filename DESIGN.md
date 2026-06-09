---
name: biasdiff
# Design system for the biasdiff GUI (Tauri desktop app).
# Authored by Google Stitch (Gemini 3.1 Pro) from the UI brief; preserved verbatim
# below, with biasdiff-specific semantic state colors and a screen-mapping appendix added.
# Provenance: Stitch project projects/8627607019004949639 — 2026-06-09.
meta:
  colorMode: LIGHT
  colorVariant: FIDELITY
  seed: '#4a6fa5'        # Slate Blue — primary brand seed
  roundness: 6px         # md (0.375rem) applied to all primary surfaces
fonts:
  headline: Hanken Grotesk
  body: Inter
  label: Geist
  japanese-fallback: ['Hiragino Kaku Gothic ProN', 'Yu Gothic', 'sans-serif']
colors:
  surface: '#f9f9fe'
  surface-dim: '#dad9de'
  surface-bright: '#f9f9fe'
  surface-container-lowest: '#ffffff'
  surface-container-low: '#f4f3f8'
  surface-container: '#eeedf2'
  surface-container-high: '#e8e8ed'
  surface-container-highest: '#e2e2e7'
  on-surface: '#1a1c1f'
  on-surface-variant: '#43474f'
  inverse-surface: '#2f3034'
  inverse-on-surface: '#f1f0f5'
  outline: '#737780'
  outline-variant: '#c3c6d1'
  surface-tint: '#395f94'
  primary: '#30568b'
  on-primary: '#ffffff'
  primary-container: '#4a6fa5'
  on-primary-container: '#edf1ff'
  inverse-primary: '#a7c8ff'
  secondary: '#50606f'
  on-secondary: '#ffffff'
  secondary-container: '#d1e1f4'
  on-secondary-container: '#556474'
  tertiary: '#705000'
  on-tertiary: '#ffffff'
  tertiary-container: '#8c6814'
  on-tertiary-container: '#fff0d9'
  error: '#ba1a1a'
  on-error: '#ffffff'
  error-container: '#ffdad6'
  on-error-container: '#93000a'
  primary-fixed: '#d5e3ff'
  primary-fixed-dim: '#a7c8ff'
  on-primary-fixed: '#001c3b'
  on-primary-fixed-variant: '#1e477b'
  secondary-fixed: '#d4e4f6'
  secondary-fixed-dim: '#b8c8da'
  on-secondary-fixed: '#0d1d2a'
  on-secondary-fixed-variant: '#394857'
  tertiary-fixed: '#ffdea4'
  tertiary-fixed-dim: '#edc065'
  on-tertiary-fixed: '#261900'
  on-tertiary-fixed-variant: '#5d4200'
  background: '#f9f9fe'
  on-background: '#1a1c1f'
  surface-variant: '#e2e2e7'
  # --- biasdiff semantic states (from the Brand prose) — the heart of the diff view ---
  adopted: '#2e7d32'            # homophone kept (Sage)
  adopted-container: '#e8f5e9'
  rejected: '#c62828'          # reading mismatch, discarded (Rose)
  rejected-container: '#ffebee'
  equal: '#737780'             # unchanged word (Neutral grey, == outline)
typography:
  display-lg:
    fontFamily: Hanken Grotesk
    fontSize: 32px
    fontWeight: '600'
    lineHeight: 40px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Hanken Grotesk
    fontSize: 24px
    fontWeight: '500'
    lineHeight: 32px
  headline-sm:
    fontFamily: Hanken Grotesk
    fontSize: 20px
    fontWeight: '500'
    lineHeight: 28px
  body-lg:
    fontFamily: Inter, system-ui, Hiragino Kaku Gothic ProN, Yu Gothic, sans-serif
    fontSize: 18px
    fontWeight: '400'
    lineHeight: 28px
  body-md:
    fontFamily: Inter, system-ui, Hiragino Kaku Gothic ProN, Yu Gothic, sans-serif
    fontSize: 16px
    fontWeight: '400'
    lineHeight: 24px
  code-label:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '500'
    lineHeight: 20px
    letterSpacing: 0.02em
  caption:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  unit: 4px
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 40px
  container-max: 1200px
  gutter: 20px
---

# biasdiff — Design System

This is the visual design system for the **biasdiff** desktop GUI: a privacy-first,
fully-local utility that collects homophone-confusion "danger words" by diffing a
correct reference sentence against an ASR transcription of it, and accumulates them
into a small ASR context-biasing dictionary.

The machine-readable tokens live in the frontmatter above. The sections below describe
the intent behind them. The canonical numeric values are the frontmatter tokens; where
the prose names a color verbally, defer to the token of the same role.

## Brand & Style

The brand personality is grounded in **Quiet Confidence**. It is a tool for precision,
designed for linguists and developers who require an unhurried, local-first experience.
The emotional response should be one of intellectual clarity and professional trust.

The design style is **Corporate / Modern** with a lean toward **Minimalism**. It
prioritizes function over flourish, utilizing a systematic approach to layout and a
restrained aesthetic. The interface remains quiet to allow the complexity of Japanese
linguistics — Kanji, Kana, and Romaji — to take center stage without visual competition.

## Colors

The palette is anchored by a deep **Slate Blue** for primary actions, signaling focus
and utility.

- **Primary (Slate Blue, `primary-container` `#4a6fa5` / `primary` `#30568b`):** CTA
  buttons, active states, and primary navigational elements.
- **Adopted / Sage (`#2e7d32`):** Used specifically for homophone matches and "adopted"
  linguistic states. Paired with a soft background (`adopted-container` `#e8f5e9`).
- **Rejected / Rose (`#c62828`):** Reserved for reading mismatches and discarded pairs.
  Paired with a soft background (`rejected-container` `#ffebee`).
- **Equal / Neutral Grey (`#737780`):** Used for "equal" words or non-varying linguistic
  data to minimize cognitive load.
- **Surface:** The background uses a clean near-white (`#f9f9fe`), while containers use a
  subtle off-white or light grey to create clear separation between input and result areas.

### Dark theme (shipped app default)

The GUI (`app/src/styles.css`) ships **dark by default** — a dark adaptation of the
palette above, not a separate identity. The light system is preserved as an opt-in: set
`<html data-theme="light">`. The app no longer auto-switches on the OS
`prefers-color-scheme`, so it stays dark unless explicitly told otherwise. `--accent` in
dark is the design system's `inverse-primary` (`#a7c8ff`); because it is a light blue,
button text on it uses the dark `--on-accent`.

| Role | CSS var | Dark (default) | Light (`data-theme="light"`) |
| --- | --- | --- | --- |
| Page background | `--bg` | `#14161b` | `#f9f9fe` |
| Raised surface (cards, inputs, buttons) | `--panel` | `#1d212a` | `#ffffff` |
| Primary text | `--text` | `#e4e7ee` | `#1a1c1f` |
| Secondary text | `--muted` | `#98a0ad` | `#43474f` |
| Primary / accent | `--accent` | `#a7c8ff` | `#30568b` |
| Text on accent | `--on-accent` | `#0a2342` | `#ffffff` |
| Border / outline | `--border` | `#2c313b` | `#c3c6d1` |
| Adopted — homophone kept | `--plus` | `#74d18d` | `#2e7d32` |
| Adopted wash | `--plus-bg` | `rgba(116,209,141,.10)` | `#e8f5e9` |
| Rejected — reading mismatch | `--minus` | `#f3a39d` | `#c62828` |
| Rejected wash | `--minus-bg` | `rgba(243,163,157,.10)` | `#ffebee` |

## Typography

This design system employs a clean, sans-serif stack optimized for Japanese legibility.

- **English / Latin characters:** **Hanken Grotesk** for headlines (sharp, contemporary),
  **Inter** for body text (maximum clarity), and **Geist** for labels and technical data
  to maintain a "developer-grade" aesthetic.
- **Japanese characters:** the stack falls back to **Hiragino Kaku Gothic ProN** and
  **Yu Gothic**.
- **Kanji / Kana hierarchy:** when Kanji and Kana are displayed together, use `body-lg`
  for primary linguistic entries and `caption` (positioned as furigana or sub-text) for
  readings.
- **Legibility:** line heights are slightly increased (1.5×–1.6×) to accommodate the
  higher visual density of Kanji.

> **Bundled locally:** the GUI ships these as variable woff2 (latin subset, SIL OFL 1.1)
> in `app/src/assets/fonts/` — no webfont CDN at runtime, in keeping with the local-only
> privacy stance. Japanese is not bundled; it uses the system gothic fallback. Roles in
> `app/src/styles.css`: `--font-head` Hanken Grotesk, `--font-body` Inter, `--font-label`
> Geist, `--font-mono` Geist Mono.

## Layout & Spacing

The layout follows a **Fixed Grid** model for desktop to ensure content remains readable
and focused, while transitioning to a fluid model for narrow widths.

- **Grid:** a 12-column grid on desktop, 4-column on mobile.
- **Structure:** distinct vertical "panes" separate Input (left) from Result / Diffing
  (right).
- **Whitespace:** generous padding within result cards (24px) ensures linguistic
  annotations never feel cramped.
- **Breakpoints:**
  - **Mobile:** < 600px (stacked view)
  - **Tablet:** 600px – 1024px (side-by-side or stacked based on content density)
  - **Desktop:** > 1024px (side-by-side panes with fixed max-width 1200px)

## Elevation & Depth

This design system avoids heavy shadows, instead using **Tonal Layers** and
**Low-contrast Outlines** to define hierarchy.

- **Base layer:** the main application background is a very light grey / near-white.
- **Surface layer:** result cards and input areas use a 1px solid border
  (`outline-variant` `#c3c6d1`) to define their boundaries.
- **Active state:** when an element is focused or selected, the border shifts to the
  primary Slate Blue with a subtle 2px outer glow (0 blur) to maintain a crisp,
  "diffing tool" aesthetic.
- **Depth:** higher elevation (modals, tooltips) uses a very soft, diffused shadow
  (0px 4px 12px, ~5% opacity charcoal) to appear naturally lifted.

## Shapes

The shape language is precise and systematic.

- **Corners:** a standard **6px (`rounded.md`, 0.375rem)** radius is applied to all
  primary containers, buttons, and input fields — a professional "softened-edge" look
  without appearing overly playful or consumer-grade.
- **Consistency:** use the same 6px radius for both internal elements (chips) and
  external containers to maintain visual harmony.

## Components

- **Buttons:** primary buttons are solid Slate Blue with white text; secondary buttons
  use a 1px border with Slate Blue text.
- **Linguistic chips:** light grey background, `code-label` typography, 6px radius. Used
  for tags, grammatical markers, and accumulated danger words.
- **Diff cards:** the core component. Adopted matches use a Sage border + background tint;
  rejected mismatches use a Rose tint; equal words stay neutral grey. Text within cards is
  perfectly aligned to allow side-by-side comparison of Kanji.
- **Input fields:** large, clean text areas with a subtly "monospaced" feel to suggest a
  "source code" for language.
- **Status indicators:** small circular pips using the Adopted / Rejected / Equal palette
  to signal alignment health at a glance.
- **Comparison rows:** alternating zebra striping in very light grey (`#f9fafb`) to assist
  horizontal scanning in the diff view.

---

## Appendix — biasdiff screen mapping

How the tokens above bind to the application's screens (see
[`biasing-dict-diff-utility-design.md`](biasing-dict-diff-utility-design.md) for the
product design). The hero is **収集 (Collect)**; **一括 (Batch)**, **辞書 (Dictionary)**,
**除外ログ (Rejects)** and **設定 (Settings)** share the same system.

| Element | Tokens |
| --- | --- |
| Session bar (count, sparkline, dict/strict toggles) | `code-label` (Geist) for counts; `primary` for the active toggle; `surface-container` ground |
| 正解文 / 認識結果 inputs | "monospaced-feel" input field; `body-lg` with the JP fallback stack; focus → primary border + 2px glow |
| 照合 button (`⌘↵`) | primary solid Slate Blue, `on-primary` text |
| **Diff view (centerpiece)** | Diff cards — Adopted = `adopted` border + `adopted-container` tint; Equal = `equal` neutral; Rejected = `rejected` + `rejected-container`; reading shown in `caption` |
| 溜まっている危険語 panel | linguistic chips — word in `body-lg`, count + reading in `caption`; newest highlighted with `primary` |
| Growth curve (Dictionary) | line in `primary`; the "頭打ち / plateau" annotation in `tertiary` |
| Privacy bar | `on-surface-variant`, quiet and persistent across every screen |
| Rejects table | zebra rows `#f9fafb`; `code-label` headers; columns 正解 / 認識 / 読み / 読み |

### State legend (the three diff outcomes)

- **Adopted** `[＋]` — replacement pair whose readings match → homophone confusion kept.
  Sage.
- **Equal** `＝` — unchanged token, quiet neutral grey. Minimizes cognitive load.
- **Rejected** `[－]` — replacement pair whose readings differ → mispronunciation / noise,
  discarded to the reject log. Rose.

> Note: in-app copy follows the existing bilingual `msg!` layer (日本語 / English). The
> Stitch mockup mixes EN/JA labels; production UI localizes fully.
