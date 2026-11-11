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
              printf '%s\\n' '{"protocol_version":1,"state":"started","message":"Owned process started"}'
              /bin/sleep 0.4
              printf '%s\\n' '{"protocol_version":1,"state":"exited","message":"Closed"}'
            else
              exit 7
            fi
            """)
    }

    func appManifest(modeVersion: Int? = 1) throws {
        var info: [String: Any] = [
            "CFBundleIdentifier": "dev.stainless.browser",
            "CFBundleExecutable": "Stainless",
            "CrProductDirName": "Stainless",
        ]
        if let modeVersion { info["StainlessManagedModesVersion"] = modeVersion }
        try plist(info, to: app.appendingPathComponent("Contents/Info.plist"))
    }

    func helperManifest(version: Int = 1) throws {
        try plist([
            "CFBundleIdentifier": "dev.stainless.browser.updater",
            "StainlessProfileChooserVersion": version,
        ], to: helper.appendingPathComponent("Contents/Info.plist"))
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
        try runtimeSelection(fixture)
        try transport(fixture)
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
            chooser.standardButton, chooser.incognitoButton, chooser.refreshButton,
            chooser.statusLabel, chooser.closeButton]
        for (index, view) in order.enumerated() {
            try expect(view.nextKeyView === order[(index + 1) % order.count],
                "Native controls must have a complete, logical keyboard cycle")
        }
        chooser.mutationBlockReason = "Synthetic pending update recovery"
        try expect(!chooser.createButton.isEnabled && !chooser.standardButton.isEnabled
            && !chooser.incognitoButton.isEnabled && chooser.refreshButton.isEnabled,
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
        if let index = CommandLine.arguments.firstIndex(of: "--snapshot"),
           CommandLine.arguments.indices.contains(index + 1),
           let content = chooser.window?.contentView {
            chooser.window?.appearance = NSAppearance(named: .aqua)
            chooser.report("Synthetic local profiles. No browser or user data was opened.")
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
    }
}
