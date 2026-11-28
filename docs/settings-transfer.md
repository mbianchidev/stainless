# Settings transfer

*Current source.* **View > Settings Transfer...** (also in the three-dot menu) exports a fixed
set of browser settings to a JSON file and imports one after a per-setting preview, for
[#8](https://github.com/stainless-browser/stainless/issues/8). It is not a profile backup,
sync or migration from another browser: use [backup and restore](profile-isolation.md#native-backup-and-clone-restore-unpublished-source)
or Chromium's importer for those. [Keyboard shortcuts](keymaps.md) have their own file.

## What a file contains

| Identifier | Setting | Values |
| --- | --- | --- |
| `restore_on_startup` | On startup | `new_tab`, `continue`, `specific_pages`, `continue_and_specific_pages` |
| `lazy_tab_restoration` | [Load restored tabs when selected](session-restore.md#load-restored-tabs-only-when-you-select-them) | `true`, `false` |
| `show_home_button` | Show Home button | `true`, `false` |
| `show_bookmarks_bar` | Show bookmarks bar | `true`, `false` |
| `warn_before_quitting` | Warn before quitting; applies to every profile of the browser | `true`, `false` |
| `ask_where_to_save_downloads` | Ask where to save downloads | `true`, `false` |
| `spell_check` | Spell check | `true`, `false` |
| `default_font_size` | Font size | integer 9 to 72 |
| `minimum_font_size` | Minimum font size | integer 0 to 24 |
| `cookie_controls` | Third-party cookies | `default`, `block_third_party_in_incognito`, `block_third_party` |
| `do_not_track` | Send Do Not Track | `true`, `false` |
| `password_breach_checks` | [Password breach checks](threat-protection.md#password-breach-checks) | `true`, `false` |
| `threat_lists` | [Threat lists](threat-protection.md); applies to every profile of the browser | `true`, `false` |
| `background_tab_memory_limit` | [Background tab memory limit](resource-overview.md#background-tab-memory-limit) | `off`, `512_mb`, `1_gb`, `2_gb`, `4_gb` |

Export writes every setting's current effective value. It never writes addresses (the startup
page list and home page are left out), history, bookmarks, passwords, cookies, accounts,
profile names or paths, keys, site lists or exceptions. Export uses the native save dialog
and Chromium's atomic file writer, and works in private windows too; the file stays on disk
until you delete it. If a current value cannot be expressed, for example a font size set
outside the range above, export fails and names the setting instead of writing a partial file.

## Preview and approval

**Import Settings...** reads one file of at most 16 KB off the main thread. Choosing a file
changes nothing. The dialog then lists each setting in the file with its current value, the
value after import and **Change**, **Same** or **Locked**. Settings not in the file are kept.

**Apply Import** is the only approval. It first rechecks that the window is still open, that
no listed setting changed since the preview and that none became locked; any mismatch rejects
the whole import with nothing changed. A setting fixed by policy or an extension is
**Locked** and blocks the import. **Cancel Import**, closing the dialog or cancelling the
file dialog changes nothing.

Import is unavailable in private windows, which would not keep the values, and in
[Safe Mode](profile-isolation.md#managed-startup-safe-mode). `threat_lists` and
`warn_before_quitting` are browser-wide settings; the others apply to this profile.

## Version 1 format

UTF-8 JSON with exactly three elements: the tag, version `1` and 1 to 14 records of
`[identifier, value]`:

```json
[
  "stainless-settings",
  1,
  [
    ["restore_on_startup", "continue"],
    ["lazy_tab_restoration", true],
    ["default_font_size", 18]
  ]
]
```

Booleans must be JSON booleans and sizes JSON integers. Objects, unknown identifiers or
values, duplicate identifiers, other versions, extra elements, comments, trailing commas and
invalid UTF-8 are rejected with a message naming the problem. Records use arrays, as in
[keymaps](keymaps.md#version-1-json-format), so a duplicated key cannot be silently ignored.

## Verification

`StainlessSettingsTransferBrowserTest.*` in `stainless_command_palette_tests` covers export
of exactly the allowlist without profile paths or URLs, a round trip, preview without changes
until approval, cancel, a stale preview, a policy-locked setting, strict parsing and the size
limit, and private windows. File dialogs are answered by a test callback with synthetic files;
run it in the [isolated macOS VM](build-macos.md#native-macos-gui-test-isolation).
