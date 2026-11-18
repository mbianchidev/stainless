# Accessibility

Stainless should be usable with a keyboard, a screen reader and other assistive technology.
This page sets the targets for contributions, describes how accessibility is checked today,
lists known limitations and explains how to report a barrier. No formal accessibility audit
has been performed, and Stainless makes no conformance claim.

## What applies where

| Surface | Owner | Target |
| --- | --- | --- |
| Web engine and ordinary browser UI (pages, tabs, toolbar, settings, DevTools) | Upstream Chromium | Preserve Chromium's accessibility. Stainless follows the latest Mac Stable release, so upstream fixes arrive through [Chromium maintenance](docs/build-macos.md#maintaining-chromium). |
| Stainless web UI, such as settings changes and `stainless://settings/localServices` | Stainless | [WCAG 2.2](https://www.w3.org/TR/WCAG22/) Level AA as the contribution target |
| Stainless native UI: command palette, keyboard shortcut editor, keymap import/export, resource overview, profile chooser and recovery, updater menu | Stainless | macOS accessibility conventions and the [Apple accessibility guidelines](https://developer.apple.com/design/human-interface-guidelines/accessibility), with full keyboard operation and VoiceOver support |
| `./tools/stainless` and other developer tooling | Stainless | Plain-text output and actionable errors that do not rely on color |
| Docs | Stainless | Descriptive link text, alt text for images and no information conveyed by color alone |

The [roadmap](docs/roadmap.md#resolved-conflicts-and-ambiguities) commits to preserving
Chromium accessibility and avoiding Stainless UI regressions; keyboard and accessibility
behavior is part of the [release gate for every phase](docs/roadmap.md#release-gate-for-every-phase).

## Guidelines for contributors

When you add or change UI:

- **Keyboard.** Every action works without a mouse. Tab and Shift+Tab reach every control in
  a logical order, Escape dismisses dialogs and focus returns to the owning window.
- **Names and roles.** Every control, including icon-only ones, has an accessible name. Reuse
  native Chromium Views or AppKit controls instead of custom drawing where possible.
- **Feedback.** Status and errors are announced to assistive technology, for example through a
  polite live region or AppKit accessibility notifications, and stay readable and selectable.
- **Focus.** Initial focus is predictable and the focus indicator stays visible.
- **Color and contrast.** Meet WCAG 2.2 AA contrast (4.5:1 for text, 3:1 for UI components
  and graphics) and never use color as the only cue.
- **Motion, zoom and reflow.** Respect reduced-motion settings, and keep web UI usable at
  200% zoom and narrow widths.
- **Labels and language.** Reuse Chromium's translated strings where they fit. New Stainless
  labels currently follow the existing untranslated resource pattern (English only).
- **Media.** Give images alt text and give demos a text description.

## How accessibility is checked today

- **Native fixtures.** The Chromium native test targets for the command palette, shortcut
  editor, keymaps, resource overview and local services drive real keyboard and focus flows,
  and several check accessible names or status announcements. They must run in an
  [isolated macOS GUI environment](docs/build-macos.md#native-macos-gui-test-isolation),
  never on a contributor's everyday desktop. See the evidence in the
  [build guide](docs/build-macos.md#browser-command-palette),
  [resource overview](docs/resource-overview.md#navigation-and-limits) and
  [local services](docs/local-services.md).
- **Updater fixtures.** The offscreen AppKit checks in `./tools/stainless test` verify the
  updater menu icon's accessible label and help text across its normal and error states
  ([details](docs/internal-urls.md#verification-boundaries)).
- **CI.** [CI](.github/workflows/ci.yml) runs the source and native helper checks on a hosted
  macOS runner. It does not build Chromium or run the browser test targets.
- **Manual checks.** Before merging UI changes, check keyboard-only operation and VoiceOver
  where you can, and say in the pull request what you checked.

## Known limitations

- **Physical keyboard and VoiceOver verification is still open.** The fixtures above are
  automated and offscreen. Human-operated checks with physical keyboard layouts and VoiceOver
  on an installed build have not been done for the command palette, shortcut editor, keymaps,
  resource overview, profile chooser and recovery, updater menu or local services.
- **Newer keyboard-first features are unreleased.** The command palette, shortcut editor,
  keymaps and resource overview exist in current source only; published previews do not have
  them ([status](docs/status.md)).
- **Google accessibility services are removed.** Cloud captions (Live Caption), image
  descriptions, downloaded Screen AI models and Google translation are unavailable. macOS
  accessibility features such as VoiceOver and dictation are separate from these removals.
  Experimental
  local replacements exist in unreleased source only; see
  [feature availability](docs/degoogling.md) and [local services](docs/local-services.md).
- **New Stainless labels are English only** until they are localized.

## Report an accessibility barrier

Open a [GitHub issue](https://github.com/mbianchidev/stainless/issues) and include:

- The Stainless version from **About Stainless** and your macOS version.
- The assistive technology and settings you use (for example VoiceOver, Full Keyboard
  Access, Zoom or increased contrast).
- Steps to reproduce, what you expected and what happened.

Use synthetic data only, and remove personal information from screenshots and recordings.
If a barrier also has security impact, report it privately through [SECURITY.md](SECURITY.md)
instead.
