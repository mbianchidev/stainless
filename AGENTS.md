# Repository guidance

## Native macOS GUI-test safety

Never run `stainless_command_palette_tests`, `stainless_local_resource_tests` or
other native Cocoa/browser GUI tests in the user's active interactive desktop
session. They can activate real windows and receive the user's typing.
`CHROME_HEADLESS=1`, `--headless=new`, virtual-display assertions, background
execution, a disposable HOME and mock Keychain are not desktop-input isolation.

Run them unchanged only on a verified dedicated macOS test host/VM with its own
GUI session and no forwarded personal keyboard input. A separate macOS user
counts only when the process actually runs in a distinct Aqua/login session;
changing HOME, using `sudo -u`, or SSH into the same active session is insufficient.
Do not suppress activation or weaken focus/keyboard assertions to manufacture
isolation. If no suitable environment is available, mark GUI verification
blocked and do not rerun it locally. Apply the same rule to aggregate commands
that launch AppKit GUI helpers. Compilation and checks that do not launch GUI
processes may continue.

Follow [native GUI-test isolation](docs/build-macos.md#native-macos-gui-test-isolation).
Treat captures from earlier unisolated runs as potentially containing user input,
not guaranteed fictional-only data; preserve user state and clean only owned fixtures.

## Chromium builds and releases

Before changing Chromium pins, downstream patches or GN arguments, and before preparing
a Stainless browser build, package or release, load or read the complete
[Chromium update skill](.github/skills/chromium-update/SKILL.md) and follow its workflow.
For check-only or plan-only requests, inspect and report without changing sources.

The baseline is the latest official Mac Stable Chromium version and full revision.
Stale or unavailable upstream evidence cannot pass the release gate. The skill is a
runbook, not a scheduled monitoring service; preserve the release-evidence boundaries in
[the Phase 1 contract](docs/phase-one.md#release-evidence).
