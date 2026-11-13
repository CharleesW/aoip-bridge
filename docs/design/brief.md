# AOIP-Bridge --- Product Design & Implementation Brief

> **Purpose:** This file is the working brief for Claude Code when
> implementing the AOIP-Bridge visual identity, design system, and user
> interface.
>
> **Approved creative direction:** Minimal Premium\
> **Theme support:** Dark and Light\
> **Brand positioning:** Professional Audio Network Infrastructure\
> **Working tagline:** Precision in Every Connection
>
> **Important:** This document defines the approved visual direction and
> engineering constraints. It is not proof that every feature described
> below already exists in the codebase. The repository is the source of
> truth for implementation status.

------------------------------------------------------------------------

## 1. Mission

Evolve AOIP-Bridge into a polished, commercially credible professional
audio-over-IP product without compromising existing functionality.

The result must feel like a purpose-built engineering application used
by audio engineers, system technicians, studios, houses of worship, and
broadcast operators---not a generic web dashboard or a decorative
concept mockup.

Prioritize:

1.  Operational clarity.
2.  Technical accuracy.
3.  Fast recognition of system state.
4.  Consistent visual language.
5.  Accessibility and legibility.
6.  Maintainable implementation.
7.  Minimal performance and dependency overhead.

Do not redesign the audio engine to achieve visual goals.

## 2. Mandatory first step: inspect the repository

Before changing files:

1.  Read `README.md`, all project-level documentation, contribution
    notes, and existing agent instructions.
2.  Inspect the full directory structure and identify the actual
    application entry points.
3.  Inspect `Cargo.toml`, workspace members, frontend manifests, build
    scripts, and packaging configuration.
4.  Identify the current UI technology, component structure, styling
    approach, asset pipeline, and runtime communication between UI and
    backend.
5.  Trace how the UI obtains state and invokes backend operations.
6.  Identify existing screens, routes, controls, settings, status
    indicators, and error handling.
7.  Run the existing build and tests where possible. Record baseline
    failures before making changes.
8.  Inspect the current application at runtime if the environment
    permits.
9.  Create a feature inventory with three categories:
    -   Implemented and verified.
    -   Present in code but not verified.
    -   Planned, absent, or unknown.

Do not assume a technology from an earlier design discussion. A
preliminary repository review suggested a Rust workspace with modules
named `core`, `engine`, `bridge`, `asio-driver`, and `mac-driver`, and
local control panels. Verify this against the current checkout. Do not
introduce Tauri, React, Vue, a new router, or another framework unless
the existing architecture demonstrably requires it.

If repository access, build prerequisites, hardware, or platform
dependencies are missing, report the limitation and continue with safe
work that can be completed.

## 3. Product and feature truth

The project's stated product direction concerns bridging conventional
audio interfaces and AES67 network audio. The README has described areas
including:

-   A bridge between host audio interfaces and AES67 networking.
-   Virtual audio driver work for Windows and macOS.
-   An Android listening/monitoring companion.
-   RTP, SDP, SAP, PTPv2, and audio stream handling.

These are project-level descriptions, not blanket confirmation that
every function is production-ready. Verify each one in source code and
documentation.

### UI feature policy

Only expose controls and status values that are backed by implemented
behavior.

For each proposed feature, classify it:

-   **Implemented:** safe to expose as an operational control.
-   **Read-only:** display only if the backend actually reports it.
-   **Experimental:** label clearly and preserve existing warnings.
-   **Planned:** do not present as working functionality.
-   **Unknown:** investigate before designing or implementing.

Potential information architecture (use only the sections supported by
the current product):

-   Overview
-   Devices / Interfaces
-   Streams
-   Routing
-   Clock / PTP
-   Performance
-   Diagnostics / Logs
-   Settings
-   Help / Documentation

Do not create fake devices, sample streams, invented telemetry,
fabricated lock states, or nonfunctional buttons in production screens.
Mock data is permitted only in isolated previews, tests, or explicitly
labeled demo fixtures.

## 4. Brand strategy

### Brand attributes

-   Precise
-   Calm
-   Technical
-   Trustworthy
-   Refined
-   Efficient
-   Professional

### Visual concept

The identity should suggest the reliable connection of independent audio
systems through a precise network bridge. Prefer abstract geometry,
structured paths, aligned nodes, and carefully balanced negative space.

Avoid: - Generic equalizer bars. - Generic sound-wave symbols. - Wi-Fi
symbols without a defensible product meaning. - Gaming aesthetics,
excessive neon, glassmorphism, and gratuitous gradients. - Overly
complex symbols that fail at small sizes. - Copying the visual identity
or interface of RME, Dante Controller, Focusrite RedNet, Avid, Bitwig,
or other products.

### Logo direction

Develop a distinctive, compact symbol that can work independently at
small sizes. Explore a visual metaphor involving two endpoints, a
bridge/transition, and a central connection or signal path.

Required logo variants: - Primary horizontal lockup. - Stacked lockup. -
Standalone symbol. - Monochrome dark and light versions. - App icon. -
System tray icon. - Favicon. - Documentation mark.

Use vector-first assets (prefer SVG). Keep geometry clean and test at
16, 24, 32, and aplik 64 px. Do not use an unreviewed text glyph as the
final logo. Do not rasterize the logo when vector rendering is
supported.

The working name is **AOIP-Bridge**. Preserve its spelling and
capitalization consistently. The working tagline is **Precision in Every
Connection**; it may be omitted from compact placements.

## 5. Design system

Implement design tokens centrally, using the existing styling
architecture. Do not scatter unrelated hex values throughout components.

The values below are the approved starting palette. Validate contrast
and refine if necessary, but document any changes.

### 5.1 Core palette

  Token                Value Intended use
  -------------- ----------- ------------------------------------------
  `ink-900`        `#171B21` Dark theme base
  `slate-800`      `#343C47` Dark theme elevated surfaces / structure
  `signal-500`     `#26A88B` Primary brand accent, active signal
  `silver-400`     `#A8B1BC` Secondary text / neutral details
  `paper-050`      `#F4F6F8` Light theme base

Create complete semantic tokens for: - Background - Surface / elevated
surface - Border / divider - Primary text - Secondary text - Muted
text - Primary action - Focus ring - Selection - Hover / pressed /
disabled - Success - Information - Warning - Error

Do not use the brand accent as the only indicator of success. Keep brand
identity and operational status semantically distinct.

### 5.2 Semantic status colors

Suggested starting values; validate contrast in both themes:

  Status                   Suggested color   Required non-color cue
  ------------------------ ----------------- -------------------------------
  Connected / healthy      `#238B68`         Check icon + text
  Synchronizing            `#3478C8`         Activity indicator + text
  Warning / PTP unlocked   `#B7791F`         Warning icon + explicit label
  Error                    `#C44747`         Error icon + explicit label
  Disconnected / unknown   Neutral gray      Disconnected icon + text

Do not claim a device is connected, a stream is active, or PTP is locked
unless the backend provides that state. Distinguish "unknown" from
"disconnected."

### 5.3 Typography

Preferred families: - **UI:** Inter (or a compatible system sans-serif
if bundling is not appropriate). - **Technical values:** JetBrains Mono
(or an existing bundled monospace family).

Use the existing font-loading strategy where possible. Evaluate font
licenses and offline packaging before adding font files.

Suggested hierarchy: - Page title: 24--28 px, semibold. - Section title:
16--18 px, semibold. - Body: 13--15 px. - Supporting labels: 12--13 px,
never too small for operational use. - Technical values: 12--14 px,
monospace. - Dense table metadata: do not go below 11--12 px without a
strong reason.

Use tabular numerals for channel numbers, sample rates, timestamps, and
measurements where supported.

### 5.4 Spacing, shape, and elevation

Use a consistent 4 px base spacing scale. Suggested spacing tokens: 4,
8, 12, 16, 20, 24, 32, 40, 48 px.

-   Prefer restrained corner radii (4--10 px).
-   Use thin borders and subtle surface separation.
-   Avoid large shadows and decorative glow.
-   Use whitespace to establish hierarchy.
-   Align labels, values, table columns, and controls precisely.
-   Do not make every section a card. Use cards only when they clarify
    grouping.

### 5.5 Icons

Use one consistent icon family and stroke weight. Prefer the icon
library already present in the repository. If none exists, evaluate a
lightweight, permissively licensed library such as Lucide before adding
it.

Icons must have accessible labels/tooltips when their meaning is not
obvious. Do not mix filled, outlined, and emoji icon styles arbitrarily.

## 6. Dark and Light themes

Both themes are required.

-   Provide a clear theme selector if the app already has
    settings/preferences architecture.
-   Persist the preference using the project's existing settings
    mechanism.
-   If the app already supports system-theme detection, preserve it and
    offer explicit override.
-   Ensure every component, chart, dialog, tooltip, table, and status
    element is theme-aware.
-   Avoid hard-coded colors in component styles.
-   Prevent flashes of the wrong theme at startup where the current
    architecture permits.
-   Verify both themes at the application's supported window sizes.

Dark theme should use layered charcoal surfaces, not pure black
everywhere. Light theme should use a soft neutral background with clear
surface boundaries, not stark white across every region.

## 7. Interaction and component standards

Build on the current component system. Reuse and improve existing
components before introducing replacements.

Standardize: - Buttons: primary, secondary, subtle, danger, icon-only. -
Inputs: text, numeric, select, toggle, checkbox. - Navigation: sidebar,
tabs, breadcrumbs if needed. - Data display: tables, key-value rows,
status badges. - Feedback: tooltip, inline validation,
toast/notification, modal/dialog. - Operational UI: device row, stream
row, channel label, connection indicator, meter only if real meter data
exists. - Empty, loading, disabled, error, and disconnected states.

Interaction requirements: - Every visible control must perform a real
action or be clearly disabled with an explanation. - Show pending state
for operations that take time. - Prevent accidental destructive
operations with confirmation. - Validate input and explain errors near
the relevant control. - Preserve keyboard navigation and visible
focus. - Avoid layout shifts when status text changes. - Use concise
technical language and consistent units.

## 8. Operational states

Use explicit labels and icons. The following names are design
vocabulary, not proof that each state exists in the backend:

-   Connected
-   Disconnected
-   Synchronizing
-   PTP Locked
-   PTP Unlocked
-   Streaming
-   Warning
-   Error
-   Unknown

For PTP, distinguish at least: - No clock information available. - Clock
discovered but not locked. - Locked/synchronized. - Lost lock / degraded
state.

Only implement distinctions supported by actual backend data. Do not
infer lock from network connectivity.

## 9. Routing matrix --- high-priority design area

The routing matrix is a signature product experience and should be
designed for clarity before decoration.

Target design scalability up to 32 × 32 channels, but do not claim 32 ×
32 routing is functional unless the engine supports it.

Requirements: - Clearly distinguish sources (rows or columns) from
destinations. - Keep channel numbers and names readable. - Make active
routes visually distinct from inactive routes. - Provide keyboard and
pointer interaction where supported. - Include accessible names for each
route control. - Make selected source/destination context obvious. -
Provide search/filter or grouping only if useful and implementable. -
Support horizontal/vertical scrolling without losing essential
headers. - Use compact but usable hit targets. - Provide clear feedback
when a route is created, changed, rejected, or removed. - Show
unavailable channels as unavailable, not as silently routable. - Do not
imply audio presence from a route connection unless signal telemetry
exists. - Do not create a routing UI that only changes local visual
state without calling the real backend operation.

For a dense matrix, consider sticky headers, channel groups, subtle
row/column banding, and a clear selected-cell treatment. Avoid excessive
grid lines and oversized colored cells.

## 10. Motion and microinteractions

Motion must communicate state, not decorate the application.

Use restrained transitions: - Short hover/focus/pressed feedback. -
Subtle page or panel transitions only if consistent with the existing
framework. - Synchronization animation only while a real synchronization
operation/state is active. - Stream activity only when real stream
activity data is available. - Loading indicators for real asynchronous
operations.

Respect reduced-motion preferences if supported. Avoid continuous
animation that consumes CPU or distracts operators. Never use animation
as the only way to communicate a state.

## 11. Accessibility and usability

Target WCAG 2.2 AA where applicable.

-   Check text and UI contrast in both themes.
-   Never use color alone to convey status.
-   Maintain visible keyboard focus.
-   Use semantic controls and accessible labels.
-   Ensure tooltips are not the only source of essential information.
-   Keep important operational labels readable at normal desktop viewing
    distance.
-   Test keyboard-only navigation.
-   Ensure dialogs have correct focus behavior.
-   Avoid flashing or rapidly blinking status indicators.

## 12. Engineering constraints --- non-negotiable

1.  Preserve the existing audio engine and signal-processing behavior.
2.  Do not rewrite core, engine, RTP, SDP, SAP, PTP, ASIO, CoreAudio, or
    driver logic for visual changes.
3.  Do not change network protocols, packet timing, sample-rate
    handling, buffer behavior, thread priorities, or real-time
    scheduling as part of UI work.
4.  Do not add blocking work to audio or real-time threads.
5.  Keep UI/backend boundaries explicit.
6.  Reuse existing APIs and message contracts.
7.  Do not change public interfaces without a demonstrated requirement
    and explicit documentation.
8.  Do not add large dependencies for minor visual effects.
9.  Check licenses, maintenance status, platform compatibility, bundle
    size, and security posture before adding dependencies.
10. Keep assets organized and avoid duplicate fonts, icons, and styles.
11. Do not commit secrets, signing keys, certificates, credentials, or
    machine-specific configuration.
12. Never force-push or publish to the default branch without explicit
    authorization.

## 13. Git workflow

-   Inspect current branch and working tree before edits.
-   Never overwrite or discard pre-existing user changes.
-   Create a dedicated feature branch for design work, for example
    `feature/visual-identity`.
-   Make small, coherent commits with descriptive messages.
-   Do not commit generated build artifacts unless the repository
    already tracks them intentionally.
-   Do not push or open a pull request unless explicitly requested.
-   Report changed files, commands run, test results, and remaining
    issues.

Suggested commit sequence: 1.
`docs: add visual identity and design system brief` 2.
`feat(ui): add semantic theme tokens` 3.
`feat(ui): establish reusable visual components` 4.
`feat(ui): refine operational overview` 5.
`feat(ui): improve routing matrix presentation` 6.
`docs(ui): document visual system and implementation`

Adjust this sequence to match actual repository structure and work
scope.

## 14. Recommended implementation phases

### Phase A --- Repository audit

Deliver: - Verified architecture map. - Existing UI inventory. - Feature
truth table. - Build/test baseline. - Risk list.

Do not redesign before this audit is complete.

### Phase B --- Foundations

Deliver: - Brand asset draft. - Central design tokens. - Dark and Light
theme infrastructure. - Typography and icon conventions. - Shared
primitives.

### Phase C --- Product screens

Prioritize existing screens: 1. Main operational overview. 2.
Device/network interface management. 3. Stream management. 4. Routing
matrix. 5. Clock/PTP view. 6. Diagnostics and logs. 7. Settings.

Only implement screens that map to real product functionality. If a
planned screen has no backend support, document it as a proposal rather
than shipping a fake screen.

### Phase D --- Refinement

-   Responsive desktop window behavior.
-   Keyboard/accessibility review.
-   Loading/error/empty states.
-   Reduced-motion behavior.
-   Performance review.
-   Visual consistency review.

### Phase E --- Verification and handoff

Run applicable: - Formatting and lint checks. - Unit tests. -
Integration tests. - Frontend build. - Rust workspace tests/build. -
Platform-specific packaging checks where available.

Compare before/after behavior. Confirm that audio engine and driver
behavior were not unintentionally changed.

## 15. Required documentation outputs

Keep documentation aligned with what is actually implemented:

-   `docs/design/brand-guidelines.md`
-   `docs/design/design-system.md`
-   `docs/design/ui-architecture.md`
-   `docs/design/feature-status.md`
-   `docs/design/implementation-notes.md`

Place files in the project's existing documentation convention if it
differs. Avoid creating duplicate documentation trees.

Required brand assets, subject to existing asset conventions: - SVG logo
variants. - App icon source and platform exports. - Tray icon. -
Favicon. - Any approved illustration or UI-specific vector assets.

Capture screenshots of the actual running application in both themes. Do
not use mockup images as evidence of implementation.

## 16. Definition of done

A design task is complete only when:

-   It matches the approved Minimal Premium direction.
-   It works in both Dark and Light themes.
-   It uses shared semantic tokens and existing architecture.
-   It is legible and accessible.
-   Every visible control has real behavior or is clearly disabled.
-   No unimplemented feature is presented as operational.
-   Existing product behavior is preserved.
-   Relevant tests/builds pass, or failures are explicitly documented.
-   Documentation reflects the shipped implementation.
-   The working tree contains no unrelated modifications.

## 17. Claude Code working protocol

At the start of each work session:

1.  Read this file and the repository's own instructions.
2.  Inspect `git status`, current branch, and recent commits.
3.  Reconfirm the actual architecture before editing.
4.  State a concise plan with files likely to change.
5.  Work in small, reviewable steps.
6.  Run focused tests after each meaningful change.
7.  Review the diff for unrelated or risky modifications.
8.  Summarize completed work, tests, limitations, and next steps.

When uncertain, inspect and ask rather than inventing. Prefer a small,
correct implementation over a broad, speculative redesign.

------------------------------------------------------------------------

## 18. Initial task for Claude Code

Begin with **Phase A --- Repository audit only**.

Do not modify application code during the initial audit.

Return: 1. Verified directory and architecture summary. 2. Actual UI
technology and entry points. 3. Current screens and components. 4.
Backend APIs/state available to the UI. 5. Feature status: implemented,
unverified, planned, unknown. 6. Build and test commands/results. 7.
Risks to audio/driver behavior. 8. A proposed implementation plan mapped
to actual files. 9. Any questions that must be resolved before
implementation.

After the audit, wait for approval before broad UI implementation. Safe
documentation-only additions may be proposed, but do not alter the
default branch.
