# Contributing to Stainless

Thanks for helping build Stainless. Bug reports, docs fixes, accessibility feedback, tests
and code are all welcome. This guide covers how to find work, set up, run the checks and
open a pull request.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

**Using a coding agent?** Have it read [AGENTS.md](AGENTS.md) before editing anything.

## Ways to contribute

- **Report a bug.** Search [existing issues](https://github.com/mbianchidev/stainless/issues)
  first, then open a new one with the Stainless version from **About Stainless**, your macOS
  version, steps to reproduce, and what you expected versus what happened. Use synthetic data
  only: no real credentials, tokens, browsing history or personal data.
- **Report an accessibility barrier.** See [ACCESSIBILITY.md](ACCESSIBILITY.md).
- **Improve the docs.** The [docs index](docs/README.md) is the map; detailed material lives
  in `docs/` and the README stays short.
- **Fix a bug or build a roadmap slice.** Work is organized under
  [epic #1](https://github.com/mbianchidev/stainless/issues/1) and the capability issues
  linked from the [roadmap](docs/roadmap.md).
- **Keep Chromium current.** Help rebase the downstream patch stack onto new Mac Stable
  releases (see [Chromium patches](#chromium-patches)).

**Security vulnerabilities do not go in public issues or pull requests.** Report them
privately as described in [SECURITY.md](SECURITY.md).

## Propose changes first

For anything larger than a small fix, comment on the owning issue or open a new one before
writing code, so scope can be agreed early. Follow the
[working conventions](docs/roadmap.md#working-conventions): each change belongs to an owning
issue under epic #1 and its approved delivery phase, and a phase is only complete when its
release evidence exists.

## Set up

Read [Building on Apple Silicon macOS](docs/build-macos.md) for the full requirements and
commands. In short:

- **Rust, updater and tooling changes** (`components/`, `tools/xtask/`) need an Apple
  Silicon Mac with Xcode or the Command Line Tools. No Chromium checkout is required.
  `./tools/stainless` installs the pinned Rust 1.99.1 if needed; with rustup,
  [`rust-toolchain.toml`](rust-toolchain.toml) selects the same toolchain with `clippy` and
  `rustfmt`.
- **Browser changes** (`chromium/`) need full Xcode 26.6 or newer with the Metal Toolchain,
  the macOS 26.5 SDK or newer, 16 GiB of memory and at least 100 GiB of free APFS disk:

  ```sh
  ./tools/stainless doctor
  ./tools/stainless bootstrap
  ./tools/stainless build
  ```

Run `./tools/stainless help` for every coordinator command.

## Before you open a pull request

Run the same checks as [CI](.github/workflows/ci.yml):

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
./tools/stainless test
cargo audit --deny warnings
```

`cargo audit` needs `cargo install cargo-audit --locked` once; CI pins version 0.23.0.

### Native GUI-test safety

`./tools/stainless test`, `package`, `smoke` and the native browser test targets (such as
`stainless_command_palette_tests` and `stainless_local_resource_tests`) launch real AppKit or
browser windows that can take focus and receive your typing. Headless flags, a disposable
`HOME` and a mock Keychain do not isolate desktop input.

Run them only on a dedicated macOS test host or VM with its own GUI session and no
forwarded personal keyboard input. If you have no such environment, do not run them on your
everyday desktop: rely on CI's hosted macOS runner for `./tools/stainless test`, and say in
the pull request which checks you could not run. Never weaken focus, keyboard or activation
assertions to make a local run look safe. Details:
[native GUI-test isolation](docs/build-macos.md#native-macos-gui-test-isolation).

Smoke helpers started by `xtask` must not outlive the run. Stop it with Ctrl-C, not
`kill -9`, then check for leftovers with `pgrep -fl "$PWD/tests/fixtures/"`.

CI checks source and native code only. It does not build Chromium, produce a DMG or replace
the installed-browser [release evidence](docs/phase-one.md#release-evidence).

## Chromium patches

Chromium history stays outside this repository. Stainless pins an exact Mac Stable revision
in [`chromium/manifest.json`](chromium/manifest.json) and applies the ordered patches in
[`chromium/patches/`](chromium/patches/). The coordinator rejects unrecognized source edits
and never resets or discards local engine changes.

Before changing Chromium pins, patches or GN arguments, follow
[Maintaining Chromium](docs/build-macos.md#maintaining-chromium) and the
[Chromium update skill](.github/skills/chromium-update/SKILL.md). Keep each patch focused,
run `./tools/stainless patch-check` and `./tools/stainless security-status`, and verify the
real browser in an isolated environment before calling an engine change ready.

## Pull requests

- Branch from `main` and keep each pull request focused on one issue.
- Add or update tests for the behavior you change. Tests and fixtures use synthetic data only;
  never use a real profile, credential, token or personal data.
- Update the docs in the same pull request when behavior, commands or limitations change.
- Keep claims honest: describe what you verified and what you could not, and never mark a
  release gate as passed without real evidence.
- Make sure CI passes, then address review feedback with follow-up commits.

## AI-assisted contributions

You are responsible for everything you contribute, including AI-assisted code, docs and
assets. Review and test your contribution, and be ready to explain what the code does and why
it is needed.

## Releases

Releases are maintainer-only. A maintainer runs `./scripts/release.sh [--dry-run] VERSION`,
which builds and packages locally, uploads a draft prerelease and pushes the tag; tag-triggered
release CI then verifies the assets, provenance, dependency audit and Chromium freshness and
publishes. See [docs/releasing.md](docs/releasing.md). Contributors do not need to run it.

## Governance

Stainless has a single maintainer and follows a benevolent-dictator model: the maintainer
reviews contributions and makes the final call on scope, roadmap, merges and releases.

## License

Stainless is licensed under the [Apache License 2.0](LICENSE). By submitting a contribution,
you agree that it is licensed under the same license, as described in section 5 of the
license. There is no separate contributor agreement. Only submit work you have the right to
contribute, and keep third-party license notices intact.
