# First-run setup

*Current source.* Installed previews do not have this yet.

The first time Stainless starts with a new user-data root, it offers a short setup sheet. It
needs no account, and **Skip** changes nothing. Every choice can be changed later in Settings.

## When it appears

Setup appears once, after the first window opens, when Chromium's first-run state applies to
a regular profile: the original preview root or a new managed Standard root. It does not
appear for Incognito, managed Incognito, Guest, Safe Mode, web-app windows, `--headless` or
`--no-first-run` launches. It cannot be reopened; use Settings instead.

## Choices

| Choice | Default | What saving does |
| --- | --- | --- |
| Address bar search engine | The current default, DuckDuckGo on a new root | Sets the selected built-in engine as the default. Disabled when a policy manages search. |
| Block known dangerous sites with on-device threat lists | The current setting, on by default | Sets the [threat-list](threat-protection.md#threat-lists) switch for every profile in this root. |
| Check saved passwords against known breaches | Off | Turns on [breach checks](threat-protection.md#password-breach-checks) for this profile. |
| Make Stainless the default web browser | Off | Asks macOS to make Stainless the default; macOS shows its own confirmation. |

**Save and Review Import Options...** saves these choices, closes setup and opens the existing
import dialog. Nothing is imported until you pick a browser there, review its data types and
confirm. Setup also notes that optional local translation, speech and image descriptions stay
off until you turn them on in Settings.

**Skip** (or Escape) closes setup without saving anything, even after you changed a control.
Search stays DuckDuckGo.

Only implemented capabilities are offered. Sync, AI assistants, extension, workspace and
developer choices will join setup with their features
([#8](https://github.com/stainless-browser/stainless/issues/8)).

## Evidence

`StainlessOnboardingBrowserTest` in `stainless_command_palette_tests` checks that **Skip**
keeps DuckDuckGo and every setting, that **Save** applies only the chosen settings, that setup
is offered only on first run, and that private windows cannot own it. Run the browser tests
only in an [isolated macOS GUI environment](build-macos.md#native-macos-gui-test-isolation).
