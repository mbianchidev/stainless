# Stainless docs

Start with the [project README](../README.md) for an overview and quickstart. Pages marked
*current source* describe unreleased work that installed previews do not have yet.

## For users

| Page | What it covers |
| --- | --- |
| [Project status](status.md) | Published previews, unreleased source and open release gates |
| [Install, update and remove](install-macos.md) | Download, checksum, unsigned-app guidance, automatic updates, backups, recovery and removal |
| [Feature availability](degoogling.md) | Removed Google services, replacement gaps and compatibility |
| [Profiles, modes and recovery](profile-isolation.md) | Managed Standard/Incognito profiles, startup Safe Mode, backups and update recovery (*current source*) |
| [Saved-history privacy](history-privacy.md) | How saved history stays local (*current source*) |
| [Internal addresses](internal-urls.md) | `stainless://` pages and compatibility (*current source*) |
| [Resource overview](resource-overview.md) | Read-only tab and process resource view (*current source*) |
| [Portable keymaps](keymaps.md) | Keyboard shortcut export, import and format (*current source*) |
| [Self-hosted Web Push](web-push-self-hosting.md) | Optional, user-controlled push server fixture (*experimental*) |
| [Accessibility](../ACCESSIBILITY.md) | Guidelines, known limitations and how to report barriers |
| [Security policy](../SECURITY.md) | Supported versions and private vulnerability reporting |

## For contributors and maintainers

| Page | What it covers |
| --- | --- |
| [Contributing guide](../CONTRIBUTING.md) | Ways to help, setup, checks and pull requests |
| [Agent instructions](../AGENTS.md) | Rules and commands for coding agents |
| [Building on macOS](build-macos.md) | Requirements, commands, GUI-test isolation, CI, caches, Chromium maintenance and packaging |
| [Chromium update skill](../.github/skills/chromium-update/SKILL.md) | Runbook for Chromium pins, patch rebases and GN changes |
| [Releasing](releasing.md) | Maintainer release process: `scripts/release.sh` and release CI |
| [Phase 1 contract](phase-one.md) | Baseline architecture, process and storage boundaries, release evidence |
| [Update verification](update-verification.md) | Clean-user procedure for installed automatic updates |
| [Local services](local-services.md) | Local runtime integration, model status and build-only probes (*experimental*) |

## Planning and history

| Page | What it covers |
| --- | --- |
| [Roadmap](roadmap.md) | Delivery phases, global decisions, non-goals and working conventions |
| [Decisions Q&A](decisions-q-and-a.md) | Historical decision record; the roadmap wins on conflicts |
| [Epic #1](https://github.com/mbianchidev/stainless/issues/1) | Execution checklist and phase status |

## Project policies

- [License](../LICENSE): Apache License 2.0
- [Security policy](../SECURITY.md)
- [Code of Conduct](../CODE_OF_CONDUCT.md)
- [Contributing guide](../CONTRIBUTING.md)
- [Accessibility](../ACCESSIBILITY.md)
