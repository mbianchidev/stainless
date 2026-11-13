import AppKit
import Foundation

private func expect(_ condition: Bool, _ message: String) throws {
    if !condition {
        throw NSError(domain: "StainlessProfileTests", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}

private func rejects(_ diagnostic: String, _ operation: () throws -> Void) throws {
    do {
        try operation()
    } catch {
        try expect(error.localizedDescription.contains(diagnostic),
            "Expected \(diagnostic), received \(error.localizedDescription)")
        return
    }
    throw NSError(domain: "StainlessProfileTests", code: 1,
        userInfo: [NSLocalizedDescriptionKey: "Expected rejection: \(diagnostic)"])
}

private struct Fixture {
    let root: URL
    let app: URL
    let helper: URL
    let executable: URL
    let recording: URL

    init() throws {
        root = FileManager.default.temporaryDirectory
            .appendingPathComponent("stainless-native-profiles-\(UUID().uuidString)", isDirectory: true)
        app = root.appendingPathComponent("Stainless.app", isDirectory: true)
        helper = app.appendingPathComponent("Contents/Helpers/Stainless Updater.app", isDirectory: true)
        executable = helper.appendingPathComponent("Contents/Resources/stainless-update")
        recording = root.appendingPathComponent("request.json")
        try FileManager.default.createDirectory(at: executable.deletingLastPathComponent(),
            withIntermediateDirectories: true)
        try appManifest()
        try helperManifest()
        try script("""
            #!/bin/sh
            set -eu
            test "$#" -eq 3
            test "$2" = --app
            test "$3" = '\(app.resolvingSymlinksInPath().path)'
            request=$(/bin/cat)
            printf '%s' "$request" > '\(recording.path)'
            if [ "$1" = profile-metadata ]; then
              case "$request" in
                *'"operation":"create"'*) printf '%s\\n' '{"protocol_version":1,"state":"metadata","message":"Created","metadata":{"protocol_version":1,"registry_version":1,"profiles":[{"id":"p-bbbbbbbbbbbbbbbb","label":"Synthetic new","profile_schema":1}]}}' ;;
                *) printf '%s\\n' '{"protocol_version":1,"state":"metadata","message":"Local","metadata":{"protocol_version":1,"registry_version":1,"profiles":[{"id":"p-aaaaaaaaaaaaaaaa","label":"Synthetic work","profile_schema":1}]}}' ;;
              esac
            elif [ "$1" = profile-launch ]; then
              case "$request" in
                *'"safe_mode":true'*) mode=',"safe_mode":true' ;;
                *) mode='' ;;
              esac
              printf '{"protocol_version":1,"state":"started","message":"Owned process started"%s}\\n' "$mode"
              /bin/sleep 0.4
              printf '{"protocol_version":1,"state":"exited","message":"Closed"%s}\\n' "$mode"
            elif [ "$1" = profile-recovery ]; then
              case "$request" in
                *'"operation":"backup"'*) printf '%s\\n' '{"protocol_version":1,"state":"backed_up","message":"Committed","backup_id":"b-aaaaaaaaaaaaaaaa"}' ;;
                *'"operation":"restore"'*) printf '%s\\n' '{"protocol_version":1,"state":"restored","message":"New identity","metadata":{"protocol_version":1,"registry_version":1,"profiles":[{"id":"p-cccccccccccccccc","label":"Synthetic recovered","profile_schema":1}]}}' ;;
                *) printf '%s\\n' '{"protocol_version":1,"state":"backups","message":"Metadata only","catalog":{"backups":[{"id":"b-aaaaaaaaaaaaaaaa","profile":{"id":"p-aaaaaaaaaaaaaaaa","label":"Synthetic work","profile_schema":1},"modified_unix_seconds":1760000000,"compatible":true,"message":"Metadata compatible; restore verifies file integrity."},{"id":"b-bbbbbbbbbbbbbbbb","profile":{"id":"p-aaaaaaaaaaaaaaaa","label":"Synthetic newer","profile_schema":1},"modified_unix_seconds":1760000001,"compatible":false,"message":"Requires a newer browser."},{"id":"b-cccccccccccccccc","profile":null,"modified_unix_seconds":null,"compatible":false,"message":"Incomplete backup retained."}],"next_after":null}}' ;;
              esac
            else
              exit 7
            fi
            """)
    }

    func appManifest(modeVersion: Int? = 1, safeModeVersion: Int? = 1) throws {
        var info: [String: Any] = [
            "CFBundleIdentifier": "dev.stainless.browser",
            "CFBundleExecutable": "Stainless",
            "CrProductDirName": "Stainless",
            "CFBundleShortVersionString": "157.0.8103.2",
            "StainlessVersion": "0.1.0-preview.2",
            "StainlessBuild": 2,
            "StainlessChannel": "preview",
            "StainlessProfileSchema": 1,
        ]
        if let modeVersion { info["StainlessManagedModesVersion"] = modeVersion }
        if let safeModeVersion { info["StainlessManagedSafeModeVersion"] = safeModeVersion }
        try plist(info, to: app.appendingPathComponent("Contents/Info.plist"))
    }

    func helperManifest(version: Int = 1, recoveryVersion: Int? = 1,
        safeModeVersion: Int? = 1) throws {
        var info: [String: Any] = [
            "CFBundleIdentifier": "dev.stainless.browser.updater",
            "StainlessProfileChooserVersion": version,
        ]
        if let recoveryVersion { info["StainlessProfileRecoveryVersion"] = recoveryVersion }
        if let safeModeVersion { info["StainlessProfileSafeModeVersion"] = safeModeVersion }
        try plist(info, to: helper.appendingPathComponent("Contents/Info.plist"))
    }

    private func plist(_ value: [String: Any], to url: URL) throws {
        try PropertyListSerialization.data(fromPropertyList: value, format: .xml, options: 0)
            .write(to: url)
    }

    func script(_ source: String) throws {
        try Data((source + "\n").utf8).write(to: executable)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: executable.path)
    }

    func request() throws -> [String: Any] {
        try JSONSerialization.jsonObject(with: Data(contentsOf: recording)) as! [String: Any]
    }
}

@main
@MainActor
struct ManagedProfilesTests {
    static func main() async throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        let fixture = try Fixture()
        defer {
            do {
                try FileManager.default.removeItem(at: fixture.root)
            } catch {
                FileHandle.standardError.write(Data("Cannot remove native fixture: \(error.localizedDescription)\n".utf8))
            }
        }
        if let index = CommandLine.arguments.firstIndex(of: "--production-helper") {
            try expect(CommandLine.arguments.indices.contains(index + 1), "Missing production fixture helper")
            try await productionRecovery(fixture, helper: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
            print("Native Swift transport and recovery controls passed against the bundled production Rust runtime in an isolated HOME, without developer tools on PATH.")
            return
        }
        try runtimeSelection(fixture)
        try transport(fixture)
        try await safeMode(fixture)
        try recoveryTransport(fixture)
        try await recoveryControls(fixture)
        try await nativeControls(fixture)
        try failures(fixture)
        print("Native managed-profile selection, protocol, lifecycle and keyboard controls passed (offscreen synthetic surface).")
    }

    private static func runtimeSelection(_ fixture: Fixture) throws {
        let app = try ManagedProfilesRuntime.browserApp(helperBundle: fixture.helper,
            arguments: ["Stainless Updater", "--app", fixture.app.path])
        let runtime = try ManagedProfilesRuntime.installed(in: app)
        try expect(runtime.executable == fixture.executable.resolvingSymlinksInPath(),
            "The chooser must select only the installed bundled helper")
        try rejects("does not belong") {
            _ = try ManagedProfilesRuntime.browserApp(helperBundle: fixture.root,
                arguments: ["Stainless Updater", "--app", fixture.app.path])
        }
        try rejects("installed app identity") {
            _ = try ManagedProfilesRuntime.browserApp(helperBundle: fixture.helper,
                arguments: ["Stainless Updater", "--app", fixture.app.path, "--no-sandbox"])
        }
        try fixture.appManifest(modeVersion: nil)
        try rejects("managed modes version 1") { _ = try ManagedProfilesRuntime.installed(in: app) }
        try fixture.appManifest()
        try fixture.helperManifest(version: 2)
        try rejects("profile chooser version 1") { _ = try ManagedProfilesRuntime.installed(in: app) }
        try fixture.helperManifest()
        try fixture.helperManifest(recoveryVersion: nil)
        try rejects("profile recovery version 1") { _ = try runtime.backups() }
        _ = try runtime.metadata()
        try fixture.helperManifest()
        let outside = fixture.root.appendingPathComponent("not-a-bundled-helper")
        try FileManager.default.moveItem(at: fixture.executable, to: outside)
        try rejects("bundled profile helper is missing") { _ = try ManagedProfilesRuntime.installed(in: app) }
        try FileManager.default.createSymbolicLink(at: fixture.executable, withDestinationURL: outside)
        try rejects("outside this app") { _ = try ManagedProfilesRuntime.installed(in: app) }
        try FileManager.default.removeItem(at: fixture.executable)
        try FileManager.default.moveItem(at: outside, to: fixture.executable)
    }

    private static func transport(_ fixture: Fixture) throws {
        let runtime = try ManagedProfilesRuntime.installed(in: fixture.app)
        let metadata = try runtime.metadata()
        try expect(metadata.profiles.count == 1 && metadata.profiles[0].label == "Synthetic work",
            "The real native transport must read the metadata-only response")
        let label = "Synthetic \"name\" / ; test"
        _ = try runtime.metadata(label: label)
        let request = try fixture.request()
        let operation = request["request"] as! [String: String]
        try expect(Set(request.keys) == ["protocol_version", "request"]
            && operation == ["operation": "create", "label": label],
            "Native create must encode inert metadata, not shell arguments")
        for mode in [ManagedProfileMode.standard, .incognito] {
            let started = DispatchSemaphore(value: 0)
            try runtime.launch(profile: metadata.profiles[0], mode: mode) { started.signal() }
            try expect(started.wait(timeout: .now()) == .success,
                "The native transport must observe the supervisor's start handshake")
            let launch = try fixture.request()
            try expect(Set(launch.keys) == ["protocol_version", "profile_id", "mode"]
                && launch["profile_id"] as? String == metadata.profiles[0].id
                && launch["mode"] as? String == mode.rawValue,
                "Launch authority must be limited to a registered ID and implemented mode")
        }
    }

    private static func waitForIdle(_ chooser: ManagedProfilesWindow) async throws {
        let deadline = Date().addingTimeInterval(5)
        while chooser.hasWork {
            try expect(Date() < deadline, "Native fixture operation did not complete")
            try await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    private static func safeMode(_ fixture: Fixture) async throws {
        let runtime = try ManagedProfilesRuntime.installed(in: fixture.app)
        let profile = try runtime.metadata().profiles[0]
        let recording = try Data(contentsOf: fixture.recording)
        let unsupportedVersions: [Int?] = [nil, 2]
        for version in unsupportedVersions {
            try fixture.appManifest(safeModeVersion: version)
            try rejects("Safe Mode version 1") {
                try runtime.launch(profile: profile, mode: .standard, safeMode: true) {}
            }
            try fixture.appManifest()
            try fixture.helperManifest(safeModeVersion: version)
            try rejects("Safe Mode version 1") {
                try runtime.launch(profile: profile, mode: .standard, safeMode: true) {}
            }
            try fixture.helperManifest()
        }
        try rejects("Standard") {
            try runtime.launch(profile: profile, mode: .incognito, safeMode: true) {}
        }
        try expect(try Data(contentsOf: fixture.recording) == recording,
            "Unsupported Safe Mode pairs must not launch a helper or silently fall back")
        let chooser = ManagedProfilesWindow(app: fixture.app)
        chooser.refreshProfiles()
        try await waitForIdle(chooser)
        try expect(chooser.safeModeButton.isEnabled
            && chooser.safeModeLabel.stringValue.contains("previous-session"),
            "The native action must explain temporary startup restrictions before launch")
        chooser.safeModeButton.performClick(nil)
        let deadline = Date().addingTimeInterval(5)
        while !chooser.statusLabel.string.contains("process started") {
            try expect(Date() < deadline, "Safe Mode did not deliver its live native acknowledgement")
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        try expect(chooser.hasWork && !chooser.standardButton.isEnabled
            && !chooser.safeModeButton.isEnabled && chooser.incognitoButton.isEnabled
            && chooser.safeModeLabel.stringValue.contains("active"),
            "Safe Mode must remain supervised and visible without allowing same-root normal relaunch")
        let request = try fixture.request()
        try expect(Set(request.keys) == ["protocol_version", "profile_id", "mode", "safe_mode"]
            && request["mode"] as? String == "standard" && request["safe_mode"] as? Bool == true,
            "Native Safe Mode grants only a constrained Standard startup request")
        chooser.closeButton.performClick(nil)
        try expect(chooser.hasWork, "Closing the chooser must not abandon Safe Mode ownership")
        try await waitForIdle(chooser)
        try expect(chooser.standardButton.isEnabled && chooser.standardButton.title == "Open Normally"
            && chooser.safeModeLabel.stringValue.contains("Open Normally")
            && chooser.statusLabel.string.contains("not saved") && chooser.keepsHelperAlive,
            "Safe exit must offer an explicit normal launch while retaining previous-session data")
        chooser.report("A different browser closed.")
        try expect(chooser.safeModeLabel.stringValue.contains("Open Normally"),
            "An unrelated session must not hide the normal-return route")
        chooser.standardButton.performClick(nil)
        try await waitForIdle(chooser)
        try expect(try fixture.request()["safe_mode"] == nil,
            "Returning to normal must remove temporary restrictions, not change stored settings")
        chooser.mutationBlockReason = "Synthetic pending update"
        try expect(!chooser.safeModeButton.isEnabled,
            "Safe Mode cannot bypass pending-update restrictions")
        chooser.mutationBlockReason = nil
        let original = try Data(contentsOf: fixture.executable)
        defer {
            do { try original.write(to: fixture.executable) }
            catch {
                FileHandle.standardError.write(Data("Cannot restore synthetic helper: \(error)\n".utf8))
            }
        }
        try fixture.script("""
            #!/bin/sh
            /bin/cat >/dev/null
            printf '%s\\n' '{"protocol_version":1,"state":"started","message":"Not safe"}'
            /bin/sleep 0.1
            printf '%s\\n' '{"protocol_version":1,"state":"exited","message":"Closed"}'
            """)
        try rejects("confirm Safe Mode") {
            try runtime.launch(profile: profile, mode: .standard, safeMode: true) {}
        }
        try fixture.script("""
            #!/bin/sh
            /bin/cat >/dev/null
            printf '%s\\n' '{"protocol_version":1,"state":"started","safe_mode":true,"message":"Started"}'
            printf '%s\\n' '{"protocol_version":1,"state":"error","message":"Synthetic Safe Mode startup failed: exit 7."}'
            exit 7
            """)
        chooser.safeModeButton.performClick(nil)
        try await waitForIdle(chooser)
        try expect(chooser.actionNeeded?.contains("exit 7") == true
            && chooser.statusLabel.string.contains("Backup / Restore")
            && chooser.recoveryButton.isEnabled && chooser.standardButton.isEnabled,
            "A failing Safe Mode launch must retain actionable diagnostics and existing recovery access")
    }

    private static func recoveryTransport(_ fixture: Fixture) throws {
        let runtime = try ManagedProfilesRuntime.installed(in: fixture.app)
        let profile = try runtime.metadata().profiles[0]
        let catalog = try runtime.backups()
        try expect(catalog.backups.count == 3 && catalog.backups[0].compatible
            && !catalog.backups[1].compatible && catalog.backups[2].profile == nil,
            "Native recovery must distinguish compatible metadata, newer snapshots and incomplete copies")
        let id = try runtime.backup(profile: profile, confirmed: true)
        var request = try fixture.request()
        var action = request["request"] as! [String: Any]
        try expect(Set(request.keys) == ["protocol_version", "request"]
            && Set(action.keys) == ["operation", "profile_id", "confirmed"]
            && action["operation"] as? String == "backup"
            && action["profile_id"] as? String == profile.id && action["confirmed"] as? Bool == true,
            "Backup authority must contain only an explicitly confirmed registered identity")
        let label = "Synthetic recovered"
        let restored = try runtime.restore(backup: id, label: label, confirmed: true)
        request = try fixture.request()
        action = request["request"] as! [String: Any]
        try expect(Set(action.keys) == ["operation", "backup_id", "label", "confirmed"]
            && action["operation"] as? String == "restore"
            && action["backup_id"] as? String == id && action["label"] as? String == label
            && action["confirmed"] as? Bool == true && restored.id != profile.id,
            "Restore transport must carry the selected backup and inert new label, never an overwrite path")
    }

    private static func waitForRecovery(_ recovery: ManagedProfileRecoveryWindow) async throws {
        let deadline = Date().addingTimeInterval(5)
        while recovery.hasWork {
            try expect(Date() < deadline, "Native recovery fixture did not complete")
            try await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    private static func recoveryControls(_ fixture: Fixture) async throws {
        let recovery = ManagedProfileRecoveryWindow(app: fixture.app)
        var quitRequests = 0
        recovery.requestBrowserQuit = { quitRequests += 1; return true }
        recovery.runningBrowserCount = 1
        recovery.refreshRecovery()
        try await waitForRecovery(recovery)
        try expect(recovery.holdsRecoveryBoundary && !recovery.backupButton.isEnabled
            && !recovery.restoreButton.isEnabled && quitRequests == 0,
            "Opening recovery must defer updates without closing a browser or copying data")
        recovery.quitBrowsersButton.performClick(nil)
        try expect(quitRequests == 1 && !recovery.backupButton.isEnabled,
            "An explicit normal-quit request is not proof that browsers released ownership")
        recovery.runningBrowserCount = 0
        try expect(!recovery.backupButton.isEnabled && !recovery.restoreButton.isEnabled,
            "A quiet boundary alone must not grant selection or consent")
        recovery.profilePicker.selectItem(at: 1)
        recovery.profileSelectionChanged()
        recovery.consentButton.performClick(nil)
        recovery.hasManagedSessions = true
        try expect(!recovery.backupButton.isEnabled,
            "Supervised sessions must finish even if Launch Services reports no browser")
        recovery.hasManagedSessions = false
        try expect(recovery.backupButton.isEnabled, "An explicit selected cold backup must be usable")
        recovery.backupButton.performClick(nil)
        try expect(recovery.hasWork && !recovery.finishButton.isEnabled,
            "Long recovery work must retain the boundary while running off the UI thread")
        try await waitForRecovery(recovery)
        try expect(recovery.statusLabel.string.contains("committed")
            && recovery.consentButton.state == .off,
            "A committed backup must be reported and must not imply consent to a restore")
        recovery.restoreNameField.stringValue = "Synthetic recovered"
        recovery.backupPicker.selectItem(at: 2)
        recovery.backupSelectionChanged()
        recovery.consentButton.performClick(nil)
        try expect(!recovery.restoreButton.isEnabled
            && recovery.detailsLabel.stringValue.contains("newer browser"),
            "Incompatible backups must remain reviewable without restore authority")
        recovery.backupPicker.selectItem(at: 3)
        recovery.backupSelectionChanged()
        try expect(!recovery.restoreButton.isEnabled
            && recovery.detailsLabel.stringValue.contains("Incomplete"),
            "Interrupted copies must remain visible and unselectable for restore")
        recovery.backupPicker.selectItem(at: 1)
        recovery.backupSelectionChanged()
        recovery.consentButton.performClick(nil)
        try expect(recovery.restoreButton.isEnabled,
            "An explicitly selected compatible backup, label and consent must enable clone-restore")
        recovery.blockReason = "Synthetic pending update recovery"
        try expect(!recovery.backupButton.isEnabled && !recovery.restoreButton.isEnabled
            && recovery.refreshButton.isEnabled && recovery.finishButton.isEnabled,
            "Pending recovery must leave read-only review and a safe exit available")
        recovery.blockReason = nil
        try snapshot(recovery.window, option: "--recovery-snapshot")
        var restored: ManagedProfile?
        recovery.restored = { restored = $0 }
        recovery.restoreButton.performClick(nil)
        try await waitForRecovery(recovery)
        try expect(restored?.id == "p-cccccccccccccccc"
            && recovery.statusLabel.string.contains("new")
            && recovery.restoreNameField.stringValue.isEmpty,
            "Clone-restore must deliver a new identity without launching it")
        let order: [NSView] = [recovery.quitBrowsersButton, recovery.profilePicker,
            recovery.backupButton, recovery.backupPicker, recovery.moreButton,
            recovery.restoreNameField, recovery.restoreButton, recovery.consentButton,
            recovery.refreshButton, recovery.statusLabel, recovery.finishButton]
        for (index, view) in order.enumerated() {
            try expect(view.nextKeyView === order[(index + 1) % order.count],
                "Every native recovery control needs a logical keyboard cycle")
        }
        try expect(recovery.finishButton.keyEquivalent == "\u{1b}",
            "Escape must leave idle recovery without closing browsers")
        recovery.finishButton.performClick(nil)
        try expect(!recovery.holdsRecoveryBoundary && !recovery.hasWork,
            "Finishing idle recovery must release only the updater deferral")
    }

    private static func productionRecovery(_ fixture: Fixture, helper: URL) async throws {
        let files = FileManager.default
        guard let homePath = ProcessInfo.processInfo.environment["HOME"] else {
            throw NSError(domain: "StainlessProfileTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "The production fixture needs an isolated HOME"])
        }
        let home = URL(fileURLWithPath: homePath).resolvingSymlinksInPath()
        try expect(home.lastPathComponent.hasPrefix("stainless-native-recovery-home-")
            && (try files.contentsOfDirectory(atPath: home.path)).isEmpty,
            "Never run the production recovery fixture against an existing user HOME")
        try files.removeItem(at: fixture.executable)
        try files.copyItem(at: helper, to: fixture.executable)
        let browser = fixture.app.appendingPathComponent("Contents/MacOS/Stainless")
        try files.createDirectory(at: browser.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("""
            #!/bin/sh
            set -eu
            data="${1#--user-data-dir=}"
            test "$3" = --stainless-managed-mode=standard
            /bin/mkdir -p "$data/Default"
            printf '157.0.8103.2' > "$data/Last Version"
            printf 'synthetic saved bookmarks' > "$data/Default/Bookmarks"
            printf ready > "$data/ready"
            i=0
            while [ ! -f "$data/stop" ] && [ "$i" -lt 600 ]; do
              i=$((i + 1))
              /bin/sleep 0.02
            done

            """.utf8).write(to: browser)
        try files.setAttributes([.posixPermissions: 0o700], ofItemAtPath: browser.path)
        let runtime = try ManagedProfilesRuntime.installed(in: fixture.app)
        let original = try runtime.metadata(label: "Synthetic original").profiles[0]
        let root = home.appendingPathComponent("Library/Application Support/Stainless Profiles")
        let data = root.appendingPathComponent("profiles/\(original.id)/data")
        var activeFixtureData = data
        let chooser = ManagedProfilesWindow(app: fixture.app)
        chooser.runningBrowserCount = 0
        chooser.refreshProfiles()
        try await waitForIdle(chooser)
        chooser.standardButton.performClick(nil)
        do {
            let deadline = Date().addingTimeInterval(5)
            while !files.fileExists(atPath: data.appendingPathComponent("ready").path)
                || !chooser.standardButton.isEnabled {
                try expect(Date() < deadline,
                    "The owned synthetic browser did not become ready. Work: \(chooser.hasWork); launch enabled: \(chooser.standardButton.isEnabled); status: \(chooser.statusLabel.string)")
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            guard let recovery = chooser.prepareRecovery() else {
                throw NSError(domain: "StainlessProfileTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Recovery admission failed"])
            }
            try expect(chooser.holdsRecoveryBoundary && chooser.keepsHelperAlive
                && !chooser.standardButton.isEnabled && !chooser.incognitoButton.isEnabled,
                "The production chooser must hold update deferral and stop new launches while recovering")
            recovery.refreshRecovery()
            try await waitForRecovery(recovery)
            recovery.profilePicker.selectItem(at: 1)
            recovery.profileSelectionChanged()
            recovery.consentButton.performClick(nil)
            try expect(!recovery.backupButton.isEnabled && chooser.hasWork,
                "A live owned browser must block cold operations even without a Launch Services entry")
            try Data().write(to: data.appendingPathComponent("stop"))
            try await waitForIdle(chooser)
            try expect(recovery.backupButton.isEnabled,
                "Normal browser exit and supervisor completion must make the native cold operation usable")
            recovery.backupButton.performClick(nil)
            try await waitForRecovery(recovery)
            try expect(recovery.failureMessage == nil && recovery.statusLabel.string.contains("committed")
                && chooser.holdsRecoveryBoundary && !chooser.hasWork,
                "The actual production backup must commit without releasing updater deferral")
            let backup = try runtime.backups().backups[0]
            let snapshotData = root.appendingPathComponent("backups/\(backup.id)/data")
            let saved = try Data(contentsOf: snapshotData.appendingPathComponent("Default/Bookmarks"))
            try expect(saved == Data("synthetic saved bookmarks".utf8), "Native backup lost synthetic profile state")
            try Data("synthetic failed live state".utf8).write(to: data.appendingPathComponent("Default/Bookmarks"))
            let label = "Synthetic \"recovered\" / ; inert"
            recovery.restoreNameField.stringValue = label
            recovery.backupPicker.selectItem(at: 1)
            recovery.backupSelectionChanged()
            recovery.consentButton.performClick(nil)
            recovery.restoreButton.performClick(nil)
            try await waitForRecovery(recovery)
            let profiles = try runtime.metadata().profiles
            guard let clone = profiles.first(where: { $0.id != original.id }) else {
                throw NSError(domain: "StainlessProfileTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "The native restore did not register a new identity"])
            }
            try expect(profiles.count == 2 && clone.label == label && clone.lastBrowser == backup.profile?.lastBrowser,
                "Native restore must preserve the captured compatibility floor in a new labeled identity")
            try expect(try Data(contentsOf: root.appendingPathComponent(
                "profiles/\(clone.id)/data/Default/Bookmarks")) == saved,
                "The new profile must contain the actual verified backup")
            try expect(try Data(contentsOf: data.appendingPathComponent("Default/Bookmarks"))
                == Data("synthetic failed live state".utf8),
                "Native clone-restore must never overwrite the original profile")
            try Data("synthetic corrupted snapshot".utf8).write(
                to: snapshotData.appendingPathComponent("Default/Bookmarks"))
            recovery.restoreNameField.stringValue = "Must not register"
            recovery.backupSelectionChanged()
            recovery.consentButton.performClick(nil)
            recovery.restoreButton.performClick(nil)
            try await waitForRecovery(recovery)
            try expect(recovery.failureMessage?.contains("corrupted or changed") == true
                && recovery.restoreNameField.stringValue == "Must not register"
                && chooser.actionNeeded != nil && (try runtime.metadata().profiles.count) == 2,
                "A real verification failure must remain actionable and preserve user input and registered identities")
            recovery.finishButton.performClick(nil)
            try expect(!chooser.holdsRecoveryBoundary && !chooser.hasWork
                && chooser.picker.titleOfSelectedItem?.contains(clone.id) == true
                && chooser.standardButton.isEnabled,
                "Finishing recovery must re-enable explicit launch of the selected new identity, not launch it automatically")
            let cloneData = root.appendingPathComponent("profiles/\(clone.id)/data")
            activeFixtureData = cloneData
            for name in ["stop", "ready"] {
                let file = cloneData.appendingPathComponent(name)
                if files.fileExists(atPath: file.path) { try files.removeItem(at: file) }
            }
            chooser.safeModeButton.performClick(nil)
            let safeDeadline = Date().addingTimeInterval(5)
            while !files.fileExists(atPath: cloneData.appendingPathComponent("ready").path)
                || !chooser.statusLabel.string.contains("Safe Mode process started") {
                try expect(Date() < safeDeadline,
                    "Production native Safe Mode failed: \(chooser.statusLabel.string)")
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            try expect(chooser.hasWork && !chooser.standardButton.isEnabled
                && chooser.safeModeLabel.stringValue.contains("active"),
                "The actual bundled runtime must retain Safe Mode ownership and its visible restrictions")
            try Data().write(to: cloneData.appendingPathComponent("stop"))
            try await waitForIdle(chooser)
            try expect(chooser.standardButton.title == "Open Normally" && chooser.keepsHelperAlive,
                "The production native safe-exit path must retain an explicit normal-return action")
            chooser.standardButton.performClick(nil)
            try await waitForIdle(chooser)
            try expect(chooser.failureMessage == nil
                && chooser.standardButton.title == "Open Standard",
                "The production helper must accept explicit normal relaunch without temporary restrictions")
        } catch {
            let originalError = error
            do {
                try Data().write(to: activeFixtureData.appendingPathComponent("stop"))
                try await waitForIdle(chooser)
            } catch {
                FileHandle.standardError.write(Data("Synthetic browser cleanup failed: \(error.localizedDescription)\n".utf8))
            }
            throw originalError
        }
    }

    private static func nativeControls(_ fixture: Fixture) async throws {
        let chooser = ManagedProfilesWindow(app: fixture.app)
        try expect(!chooser.standardButton.isEnabled && !chooser.incognitoButton.isEnabled,
            "An empty chooser must never open the preview identity")
        try expect(chooser.window?.initialFirstResponder === chooser.nameField,
            "Empty-profile keyboard focus must start at explicit creation")
        chooser.refreshProfiles()
        try await waitForIdle(chooser)
        try expect(chooser.picker.numberOfItems == 1 && chooser.standardButton.isEnabled,
            "Refresh must populate the native profile picker through the bundled transport")
        chooser.nameField.stringValue = "Synthetic new"
        chooser.createButton.performClick(nil)
        try await waitForIdle(chooser)
        try expect(chooser.picker.numberOfItems == 2 && chooser.picker.indexOfSelectedItem == 1
            && chooser.nameField.stringValue.isEmpty,
            "Explicit create must select only the newly returned local profile")
        try expect(chooser.standardButton.keyEquivalent == "\r"
            && chooser.closeButton.keyEquivalent == "\u{1b}",
            "Return opens Standard and Escape closes the chooser")
        let order: [NSView] = [chooser.picker, chooser.nameField, chooser.createButton,
            chooser.standardButton, chooser.incognitoButton, chooser.safeModeButton,
            chooser.recoveryButton, chooser.refreshButton,
            chooser.statusLabel, chooser.closeButton]
        for (index, view) in order.enumerated() {
            try expect(view.nextKeyView === order[(index + 1) % order.count],
                "Native controls must have a complete, logical keyboard cycle")
        }
        chooser.mutationBlockReason = "Synthetic pending update recovery"
        try expect(!chooser.createButton.isEnabled && !chooser.standardButton.isEnabled
            && !chooser.incognitoButton.isEnabled && !chooser.recoveryButton.isEnabled
            && chooser.refreshButton.isEnabled,
            "Pending recovery must leave metadata refresh available without launch authority")
        chooser.mutationBlockReason = nil
        chooser.incognitoButton.performClick(nil)
        let deadline = Date().addingTimeInterval(5)
        while !chooser.standardButton.isEnabled {
            try expect(Date() < deadline, "Start handshake did not restore native controls")
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        try expect(chooser.hasWork && chooser.keepsHelperAlive,
            "A running managed browser must retain native helper supervision")
        chooser.closeButton.performClick(nil)
        try expect(chooser.hasWork, "Closing the chooser must not abandon the foreground owner")
        try await waitForIdle(chooser)
        try expect(chooser.statusLabel.string.contains("browser closed"),
            "The actual helper exit must be surfaced")
        try snapshot(chooser.window, option: "--snapshot")
        try fixture.script("""
            #!/bin/sh
            /bin/cat >/dev/null
            printf '%s\\n' '{"protocol_version":1,"state":"error","message":"Synthetic launch failure: this profile is already owned."}'
            exit 1
            """)
        chooser.standardButton.performClick(nil)
        try await waitForIdle(chooser)
        try expect(chooser.statusLabel.string.contains("Synthetic launch failure")
            && chooser.standardButton.isEnabled && !chooser.hasWork && chooser.keepsHelperAlive,
            "Launch failures must be actionable in the native surface, not successful fallback launches")
        chooser.report("A different managed browser closed.")
        try expect(chooser.statusLabel.string.contains("Synthetic launch failure"),
            "An unrelated successful exit must not erase a retained profile failure")
    }

    private static func snapshot(_ window: NSWindow?, option: String) throws {
        if let index = CommandLine.arguments.firstIndex(of: option),
           CommandLine.arguments.indices.contains(index + 1),
           let content = window?.contentView {
            window?.appearance = NSAppearance(named: .aqua)
            content.layoutSubtreeIfNeeded()
            guard let bitmap = content.bitmapImageRepForCachingDisplay(in: content.bounds) else {
                throw NSError(domain: "StainlessProfileTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Cannot render the offscreen native fixture"])
            }
            content.cacheDisplay(in: content.bounds, to: bitmap)
            let bounds = CGRect(x: 0, y: 0, width: bitmap.pixelsWide, height: bitmap.pixelsHigh)
            guard let layer = bitmap.cgImage,
                  let composite = CGContext(data: nil, width: bitmap.pixelsWide, height: bitmap.pixelsHigh,
                    bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                throw NSError(domain: "StainlessProfileTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Cannot composite the offscreen native fixture"])
            }
            composite.setFillColor(CGColor(gray: 0.94, alpha: 1))
            composite.fill(bounds)
            composite.draw(layer, in: bounds)
            guard let image = composite.makeImage(),
                  let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
                throw NSError(domain: "StainlessProfileTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Cannot encode the offscreen native fixture"])
            }
            try png.write(to: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
        }
    }

    private static func failures(_ fixture: Fixture) throws {
        let runtime = try ManagedProfilesRuntime.installed(in: fixture.app)
        try fixture.script("""
            #!/bin/sh
            /bin/cat >/dev/null
            printf '%s\\n' '{"protocol_version":1,"state":"error","message":"This profile is already owned. Close its browser before retrying."}'
            exit 1
            """)
        try rejects("already owned") { _ = try runtime.metadata() }
        for (reply, diagnostic) in [
            (#"{"protocol_version":2,"state":"metadata","message":"Future"}"#, "Unsupported native profile protocol"),
            (#"{"protocol_version":1,"state":"metadata","message":"Future","metadata":{"protocol_version":1,"registry_version":2,"profiles":[]}}"#, "Unsupported or invalid profile metadata"),
            (#"{"protocol_version":1,"state":"metadata","message":"Invalid","metadata":{"protocol_version":1,"registry_version":1,"profiles":[{"id":"../preview","label":"Synthetic","profile_schema":1}]}}"#, "Unsupported or invalid profile metadata"),
            (#"{"protocol_version":1,"state":"started","message":"Not metadata"}"#, "unexpected lifecycle result"),
            (#"not JSON"#, "invalid output"),
        ] {
            try fixture.script("#!/bin/sh\n/bin/cat >/dev/null\nprintf '%s\\n' '\(reply)'")
            try rejects(diagnostic) { _ = try runtime.metadata() }
        }
        try fixture.script("""
            #!/bin/sh
            /bin/cat >/dev/null
            /bin/dd if=/dev/zero bs=1024 count=4096 2>/dev/null
            """)
        try rejects("output limit") { _ = try runtime.backups() }
    }
}
