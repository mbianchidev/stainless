# Portable browser keymaps

This unpublished source slice adds **Export keymap...** and **Import keymap...** to
**View > Keyboard Shortcuts...** on macOS. The three-dot menu and the palette's
**Keyboard Shortcuts...** button keep the same fixed editor access. It is not a
profile backup, settings migration, cloud sync, macro system or cross-platform
compatibility promise.

## Export and privacy

Export uses the native save dialog, including its overwrite confirmation. It writes
one complete plaintext JSON file using Chromium's atomic file writer. A failed
write reports an error, not success; an existing destination is not deliberately
truncated before a complete replacement is ready. This is not a guarantee against
filesystem/hardware failure or a system crash.

The file contains all 20 supported actions' **effective shortcut states**:
default, explicitly unbound, or custom. It does not serialize the profile's raw
preferences, ignored values, native alias inventory or macOS App Shortcuts.
No profile ID, label, directory, account, browsing record, secret, device key,
OS configuration, command ID, URL or executable content is exported.
Invalid saved mappings must be recovered with the editor's confirmed reset before
export; they are not silently converted into a successful empty export.

| Context | Exported state and import lifetime |
| --- | --- |
| Normal profile | Current profile-local bindings; approved imports use the same unsynced preference and normal persistence as the editor. Other profiles are untouched. |
| Inherited Incognito | Its current effective overlay, initially inherited from only its own normal identity. A private reset exports defaults, not ignored normal overrides. Imports remain in that OTR profile's memory until its last private window closes. |
| Fresh managed Incognito | Defaults unless explicitly edited/imported in that fresh private context. No normal identity's stores are copied; imports do not persist into its backing normal profile. |
| Startup Safe Mode | Only active defaults are exported. Saved overrides are ignored and never included. Import and other mutation controls remain read-only; saved values are not reset or deleted. |

**Explicit export writes a real file even in Incognito.** That file remains on disk
after private windows close. Choose its destination deliberately and delete it
yourself when no longer wanted. No background transfer, network request, automatic
file deletion or new profile-store authority is added.

## Preview and approval

Import opens the native single-file picker, then reads a bounded local file on a
worker thread. Selecting a file does **not** approve changes.

The existing native action table gains an **After import** column. Every supplied
action shows **Change** or **Same**, its proposed default/custom/unbound state and
the native shortcut label where applicable. **Default (all aliases)** restores
Chromium's native aliases; **Unbound (remove all)** removes every binding for that
action. Omitted actions show **Keep current**. Row tooltips retain the full proposed
text. Conflicts identify the affected action and block the entire import.

Only **Apply import** approves the displayed set. **Cancel import**, Close, Escape
or a canceled picker makes no shortcut change. Single-action editing/reset and
another transfer are unavailable while a file operation or preview is pending.
Invalid input leaves the editor usable and preferences unchanged.

A partial file changes only its listed actions. A full exported file lists every
supported action, so applying it can replace all 20 states; all those changes are
previewed. Two listed actions can exchange bindings in one approved transaction.
There is no silent removal, swap or overwrite of an omitted action.

Apply checks the same owning browser/profile, current preference snapshot,
supported bindings, native/static/default-alias reservations, active high-priority
extension handlers and macOS symbolic conflicts again. Changed preferences make
the preview stale: cancel and reopen the file to review fresh changes. A conflict,
read-only store, ineligible/closing owner or Safe Mode rejects the whole operation.
The final dictionary is set once, not through a series of partially applied edits.
Normal menu/action shortcut observers update labels and dispatch.

This retains the editor's macOS App Shortcuts restriction and normal command
availability guards. An imported binding cannot enable an unavailable command,
escape forced Incognito/Safe Mode, register an OS/global hook or grant renderer,
subprocess or arbitrary filesystem authority.

## Version 1 JSON format

UTF-8 JSON, at most **65,536 bytes inclusive**, with this exact root tuple:

```json
[
  "stainless-keymap",
  1,
  [
    ["new_tab", "custom", 75, 22],
    ["downloads", "default"],
    ["command_palette", "unbound"]
  ]
]
```

The root has exactly three elements: the format tag, integer version `1`, and
1 to 20 action records. An action may occur only once. Records have exactly one
of these shapes:

| Record | Meaning |
| --- | --- |
| `["action", "default"]` | Remove that action's override and restore its current native default plus aliases. |
| `["action", "unbound"]` | Store an explicit empty override, suppressing the primary binding and every native alias, even for an action with no default primary binding. |
| `["action", "custom", key, modifiers]` | Replace all native aliases with one validated nonempty accelerator. Both numbers must be JSON integers, not strings, booleans or fractional values. |

`key` and `modifiers` use the existing Chromium shortcut preference representation,
not a new shortcut-string parser. For newly assigned combinations, `key` is `48`
through `57` (digits) or `65` through `90` (letters). Command is `16`, Shift is `2`
and Control is `4`, giving supported custom modifier masks `16`, `18`, `20` or
`22`. The example is Command+Control+Shift+K. The shared editor validator also
recognizes an action's exact native primary default, including defaults that
cannot be newly assigned as general custom combinations. A custom record still
suppresses aliases; use `default` to restore them. Empty custom combinations are
invalid; use `unbound`.

Array-only records intentionally avoid ambiguous duplicate JSON object keys.
Objects, extra elements, unknown actions/states, duplicate actions, unsupported
versions/types, excessive nesting, comments, trailing commas, invalid UTF-8 and
unsupported or reserved accelerators are rejected. Configuration/platform
conflicts may be displayed in a non-applicable preview so the user can see what
was proposed. Neither malformed data nor an unavailable file becomes an empty
successful import. The only paths used come from the user's native picker;
file contents never select another source, destination or command.

The supported action identifiers are:

```text
new_tab             new_window          new_incognito_window
close_tab           duplicate_tab       restore_tab
find                back                forward
reload              zoom_in             zoom_out
zoom_reset          focus_location      bookmark_tab
bookmark_manager    history             downloads
settings            command_palette
```

These are a bounded allowlist, not arbitrary Chromium command IDs. Defaults are
resolved against the receiving browser, rather than copying platform-private
aliases. Layout, macOS, extension and browser changes can make a custom shortcut
incompatible; current validation remains authoritative. Version 1 does not
promise Windows/Linux, other keyboard layouts or future native-default stability.

## Verification boundaries

The existing native Chromium fixture target exercises the actual modified editor,
production parser/validator, real isolated file I/O, preference observers and
native accelerator dispatch. Picker responses are controlled test callbacks:
the suite does not drive the user's desktop or installed native dialogs. Its
synthetic profiles and mock Keychain never use personal browsing data.

The keymap journeys cover independent hand-written input, export/import into
another synthetic profile, preview/apply/cancel/close, aliases/defaults/unbound
states, strict parsing and size boundaries, conflicts, stale approval, single
mutation, I/O errors, owner lifetime, private state and restart/Safe Mode behavior.
These are source-informed native integration checks, not a blind black-box audit.
No meaningful existing tests are removed.

Build/run commands and actual source/browser evidence are in the
[macOS build guide](build-macos.md#portable-keymap-source-evidence).
Physical keyboard/layout/VoiceOver, installed native file-picker workflows,
clean-Mac installation, authorized updates, licensing/disclosure and the complete
Phase 1/#4/#6/#7/#8 gates remain separate. This slice publishes no browser or
installer and does not change installed previews.
