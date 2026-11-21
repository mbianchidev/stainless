# Repository guidance

## Local cleanup

**Very important.**

- Keep the occupied space to the minimum viable for working on the project and ensure to clean after yourself after you are done pushing code / waiting for CI / addressing comments.
- Smoke helpers that `xtask` starts (such as `tests/fixtures/https_server.py`) must not outlive the run. Stop `xtask` with Ctrl-C (it reaches the whole foreground process group) or SIGTERM to its process group, never SIGKILL or a hard tool timeout: `xtask` has no signal handler, and `ProcessGuard` stops a helper's whole process tree only when `xtask` exits normally. `scripts/release.sh` stops its steps' process groups itself. After every smoke or test run, including failed or interrupted ones, run `pgrep -fl "$PWD/tests/fixtures/"` and `kill` any leftover PID from your worktree. Confirm none remain before ending the session.
- Chromium cache clones (`~/Library/Caches/StainlessBuild*-<session-id>`) are 50 to 90 GB each. Reuse your session's clone instead of making new ones per experiment, and list the clones you created and still need in your session summary. When no more builds are planned (PR merged, abandoned, or handed off), delete your own clones after `lsof | grep <clone>` shows no users. Never delete the shared `StainlessBuild` base, another session's clone, or a clone a live session lists as protected without explicit user approval.
- Copilot git copies (`~/Library/Caches/github-copilot-git-<version>`): delete an older version only when `ps eww -axo command | grep -F <dir-name> | grep -v grep` prints nothing; running sessions keep them on PATH even when `lsof` shows nothing open. Always keep the newest version.
- Stale worktrees under `~/workspace/copilot-worktrees/stainless/`: remove one only when its PR is merged or closed, `git status --porcelain` is empty, nothing is unpushed, and no live session or process uses it. Use `git worktree remove` without `--force`, never on your own worktree. Prefer archiving the owning Copilot session, which removes the worktree too.
- Report reclaimed space as the change in `diskutil info / | grep "Free Space"`; `du` overcounts APFS clones.

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

## Developer commands and docs

Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [docs index](docs/README.md) first. Setup,
requirements, CI and packaging are in the [macOS build guide](docs/build-macos.md).

Run the same checks as [CI](.github/workflows/ci.yml) before handing off:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
./tools/stainless test   # launches AppKit helpers: follow the GUI-test safety rules above
cargo audit --deny warnings
```

- `./tools/stainless help` lists coordinator commands. `doctor`, `bootstrap` and `build`
  need full Xcode; Rust/updater work and `test` do not need a Chromium fetch. `build`
  ends with offscreen AppKit updater/profile helper checks, so it falls under the GUI-test
  safety rules too; `compile` builds only the browser and skips them. The opt-in
  [compile cache](docs/compile-cache.md) is local and never used for packages.
- Tests and fixtures use synthetic data only. Never use real credentials, tokens, browsing
  history or the user's profiles.
- Keep the README short and put detail in `docs/`, linked from `docs/README.md`.
- Releases are maintainer-only through `scripts/release.sh` and release CI, documented in
  [docs/releasing.md](docs/releasing.md). Do not push tags or publish releases unless the
  maintainer explicitly asks.
