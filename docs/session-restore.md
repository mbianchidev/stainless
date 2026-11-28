# Session restore

*Current source.* Two additions to Chromium's session restore, part of
[#8](https://github.com/stainless-browser/stainless/issues/8). They do not change installed
previews.

## Load restored tabs only when you select them

**Settings > On startup > Load restored tabs only when you select them** is off by default.
It is a per-profile, unsynced setting (`stainless.lazy_tab_restoration`).

When it is on and Stainless continues where you left off, only each window's selected tab
loads at startup. The other restored tabs keep their titles, favicons from the local favicon
database and back/forward history, and load the first time you select them. This saves
memory, CPU and network use when a session has many tabs.

With the setting off, Chromium's background loading policy loads every restored tab, as
before. The setting does not affect **Reopen closed tab** or
[Safe Mode's session review](#review-the-previous-session-in-safe-mode), and it can be moved
between profiles with [settings transfer](settings-transfer.md).

## Review the previous session in Safe Mode

[Safe Mode](profile-isolation.md#managed-startup-safe-mode), whether opened explicitly or
[automatically after failed startups](profile-isolation.md#automatic-safe-mode-after-repeated-failed-startups),
does not restore the previous session but keeps its files. **History > Review Previous
Session...** (also in the three-dot menu) appears only in Safe Mode, in normal windows of a
regular profile. The Safe Mode notice points to it.

1. The review reads the newest complete session file in the profile's `Sessions` folder off
   the main thread. It opens no file for writing and moves or deletes nothing: Chromium's own
   session reader would delete older session files on startup, so the review uses a read-only
   path instead.
2. It lists each normal window of that session and its tabs by title, with the address in
   the tooltip and accessible description. Every tab starts selected. **Select All** and
   **Select None** change the whole list.
3. **Reopen Selected** opens the selected tabs in new windows, one per original window, with
   their back/forward history. Tab groups, splits and app windows are not recreated. With
   nothing selected the button is disabled. **Cancel** reopens nothing.

If no session was saved, the review says so. If part of the file cannot be read, it lists the
readable tabs and says that the rest could not be read; an unreadable file is kept unchanged.

Safe Mode still saves nothing about its own tabs, so reopened tabs are not part of the next
normal session unless they are open when you quit normally afterwards. Reopening a tab that
caused the failed startups can make it fail again; reopen tabs a few at a time if unsure.

## Verification

`stainless_command_palette_tests` covers both features in the
[isolated macOS VM](build-macos.md#native-macos-gui-test-isolation):

- `StainlessLazyTabRestoreBrowserTest.BackgroundTabsLoadWhenSelected` saves three synthetic
  tabs, restarts and checks that only the selected tab loads until another is selected.
  `StainlessEagerTabRestoreBrowserTest.BackgroundTabsLoadByDefault` checks the default.
- `StainlessSessionReviewBrowserTest.*` saves a synthetic session, restarts in automatic Safe
  Mode, reopens two of its three tabs and checks that every session file is byte-for-byte
  unchanged. It also covers an empty selection and a profile without a previous session.

All of them passed on 2026-11-28 in an isolated Tart macOS 26.6 VM, with the 49-patch browser
built from Chromium 159.0.8154.34, as part of a 148-test native run with no failures.
