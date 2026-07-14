# Deskemy — Visual Overhaul Plan

Forward-looking plan for theming / visual work. **Deferred** — land the core v1.2+
features first, then implement. This document is implementation-ready so a future
session doesn't have to re-derive anything.

Related: [PLAN.md](../PLAN.md) (baseline), [ROADMAP.md](../ROADMAP.md) (features).

Legend: 🔜 next · ⏳ planned · ✅ proven · 🔑 key decision

---

## 1. Custom themes from a color palette  ⏳ (mapping ✅ proven)

**Goal.** Let a user create a full app theme by pasting a color palette (a
Coolors / Tailwind v4 export of *numbered scales*, e.g. `--color-ash-grey-50…950`)
and picking which scale plays each semantic role. Both dark and light variants are
generated from the one palette.

**Feasibility — confirmed.** Deskemy already ships a semantic token layer in
[src/app.css](../src/app.css): an `@theme` block defines `--color-*` roles, and the
light theme works purely by *overriding those same CSS vars* under
`:root[data-theme="light"]`. A custom theme is the identical mechanism with
computed values — no architectural change.

**Proof of concept.** An interactive "Theme Studio" was built (artifact) that loads
the sample palette, auto-maps it, renders real Deskemy chrome (sidebar, cards,
control bar, components) in dark + light, and outputs the generated CSS. The
mapping below is exactly what it implements. The ash-grey + teal sample lands as a
calm foresty dark theme; the slightly-green neutral is what gives it character.

### 1a. The core problem: scales → semantic roles

The palette is *raw numbered scales*; our theme is *semantic roles*. The user (or
auto-detect) assigns three scales:

- **Neutral** → surfaces, text, lines (the scale that matters most; an all-saturated
  palette with no neutral hue yields a heavily tinted UI — sometimes wanted, often not).
- **Primary** → primary / containers / inverse (buttons, active states, accents).
- **Secondary** → active-nav accent + links.

Auto-detect default: **neutral = least-saturated** scale (by the `500` shade),
**primary = most-saturated**, **secondary = next**. User can override all three.

### 1b. The mapping (implementation-ready)

19 roles per variant. `N`/`P`/`S` = the chosen neutral/primary/secondary scales;
`mix(a,b,t)` = linear RGB interpolation; fixed values in `mono`.

| Role | Dark | Light |
|---|---|---|
| `background` | `N-950` | `N-50` |
| `surface` | `N-900` | `#ffffff` |
| `surface-container-lowest` | `N-950` | `#ffffff` |
| `surface-container-low` | `N-900` | `N-100` |
| `surface-container` | `mix(N-900,N-800,.5)` | `mix(N-100,N-200,.6)` |
| `surface-container-high` | `N-800` | `N-200` |
| `surface-container-highest` | `mix(N-800,N-700,.55)` | `mix(N-200,N-300,.5)` |
| `on-surface` | `N-100` | `N-900` |
| `on-surface-variant` | `N-300` | `N-700` |
| `outline` | `N-500` | `N-500` |
| `outline-variant` | `mix(N-800,N-700,.4)` | `N-300` |
| `border` | `mix(N-900,N-800,.6)` | `N-200` |
| `primary` | `P-300` | `P-700` |
| `primary-container` | `P-600` | `P-500` |
| `on-primary-container` | `P-50` | `#ffffff` |
| `inverse-primary` | `P-700` | `P-700` |
| `secondary-container` | `S-500` | `S-600` |
| `accent-blue` | `S-400` | `S-600` |
| `error` | `#ff6b6b` | `#ba1a1a` |

**Note on `error`:** semantic "danger" is intentionally *not* re-hued — most
palettes have no red, and a green "error" is a bug, not a feature. Keep a fixed red
(or, later, let the user optionally nominate a warm scale for it).

Palette parse regex: `/--color-([a-z0-9-]+?)-(50|100|…|950)\s*:\s*(#[0-9a-fA-F]{6})/g`
→ group by name; keep scales with ≥8 of 11 shades present.

### 1c. Implementation plan

- **Config (no DB migration).** Theme lives in `config.json`, not SQLite. Add an
  optional `custom_theme` to `AppConfig` (`config.rs`): the palette text + role
  assignments (regenerate on load), or the precomputed dark/light token maps.
  Extend the theme value beyond `dark|light|system` to allow `custom`.
- **Apply.** `applyTheme` in [app.svelte.ts](../src/lib/stores/app.svelte.ts): when
  a custom theme is active, generate the two token maps and write them as CSS vars —
  inject a `<style>` element defining `:root { --color-… }` (dark) and
  `:root[data-theme="light"] { --color-… }` (light), overriding the `@theme`
  defaults. The dark/light/system switch still selects which variant shows.
- **Settings UI.** New "Appearance" block: the existing Dark/Light/System plus a
  **Custom** option that reveals paste-palette + Neutral/Primary/Secondary pickers +
  a live preview swatch strip, and **Save**. Ship 1–2 curated **presets** (bake the
  ash-teal sample in as "Ash Teal").
- **Reuse the Studio JS** — the `parse()`, `generate()`, and color helpers from the
  POC artifact are the reference implementation; port them to a `lib/theme.ts`.

### 1d. Scope decision (🔑 pick when we build)

- **Full editor (recommended)** — paste + role pickers + live preview + save, plus
  a few curated presets. Most flexible; medium build.
- **Presets only** — bake a handful of curated themes into the Dark/Light/System
  picker; no paste-your-own. Simpler; not user-extensible.

### 1e. Risks / open questions

- **Contrast validation.** Auto-check `on-surface` vs `surface` and
  `on-primary-container` vs `primary-container` (WCAG-ish ratio); warn if a chosen
  palette produces low-contrast text. The `500` shade of a "neutral" that's actually
  vivid will muddy surfaces.
- **Light-mode primary contrast.** `primary-container = P-500` with white text can be
  marginal for pale scales; may need a per-variant shade nudge or an auto lighten/darken.
- **The airspace video pane** is unaffected (mpv renders its own surface); only DOM
  chrome re-themes. Good — no player interaction.

### 1f. Sample palette (the request that kicked this off)

Ash-grey (neutral) · muted-teal · deep-teal (primary) · dark-slate-grey ·
charcoal-blue (secondary) — a Coolors export. Kept in the Studio POC as the default.

---

## 2. Later visual candidates (unscoped)

- Density toggle (comfortable / compact) via a spacing-token scale.
- Motion pass — tasteful page/transition polish (respect `prefers-reduced-motion`).
- Accent-tuned neutrals audit — ensure greys carry a slight brand-hue bias.
