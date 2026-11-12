import AppKit
import Foundation

struct ManagedProfile: Decodable, Equatable, Sendable {
    let id: String
    let label: String
    let profileSchema: UInt32

    enum CodingKeys: String, CodingKey {
        case id, label
        case profileSchema = "profile_schema"
    }
}

struct ManagedProfileMetadata: Decodable, Sendable {
    let protocolVersion: UInt32
    let registryVersion: UInt32
    let profiles: [ManagedProfile]

    enum CodingKeys: String, CodingKey {
        case profiles
        case protocolVersion = "protocol_version"
        case registryVersion = "registry_version"
    }

    func validated() throws -> Self {
        guard protocolVersion == 1, registryVersion == 1, profiles.count <= 256,
              Set(profiles.map(\.id)).count == profiles.count,
              profiles.allSatisfy({
                  $0.id.utf8.count == 18 && $0.id.hasPrefix("p-")
                      && $0.id.utf8.dropFirst(2).allSatisfy {
                          (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
                      }
                      && $0.profileSchema == 1 && !$0.label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                      && $0.label.utf8.count <= 128
                      && $0.label.rangeOfCharacter(from: .controlCharacters) == nil
              }) else {
            throw profileError("Unsupported or invalid profile metadata. Keep the registry and use a compatible browser.")
        }
        return self
    }
}

enum ManagedProfileMode: String, Encodable, Sendable {
    case standard, incognito
}

private func profileError(_ message: String) -> NSError {
    NSError(domain: "StainlessProfiles", code: 1,
        userInfo: [NSLocalizedDescriptionKey: message])
}

private struct ProfileReply: Decodable {
    let protocolVersion: UInt32
    let state: String
    let message: String
    let metadata: ManagedProfileMetadata?

    enum CodingKeys: String, CodingKey {
        case state, message, metadata
        case protocolVersion = "protocol_version"
    }
}

private struct ProfileReplyReader {
    let handle: FileHandle
    private var bytes = Data()
    private var total = 0

    init(_ handle: FileHandle) { self.handle = handle }

    mutating func next() throws -> ProfileReply? {
        while true {
            if let end = bytes.firstIndex(of: 10) {
                let line = Data(bytes[..<end])
                bytes.removeSubrange(...end)
                do {
                    let reply = try JSONDecoder().decode(ProfileReply.self, from: line)
                    guard reply.protocolVersion == 1 else {
                        throw profileError("Unsupported native profile protocol. Install a compatible browser.")
                    }
                    return reply
                } catch let error as NSError where error.domain == "StainlessProfiles" {
                    throw error
                } catch {
                    throw profileError("The bundled profile helper returned invalid output. Repair the app installation.")
                }
            }
            guard let chunk = try handle.read(upToCount: 4096), !chunk.isEmpty else {
                guard bytes.isEmpty else {
                    throw profileError("The bundled profile helper returned an incomplete result.")
                }
                return nil
            }
            total += chunk.count
            guard total <= 2 * 1024 * 1024 else {
                throw profileError("The bundled profile helper exceeded its metadata output limit.")
            }
            bytes.append(chunk)
        }
    }
}

struct ManagedProfilesRuntime: Sendable {
    let app: URL
    let executable: URL

    static func browserApp(helperBundle: URL, arguments: [String]) throws -> URL {
        guard arguments.count == 3, arguments[1] == "--app",
              arguments[2].hasPrefix("/") else {
            throw profileError("The browser did not provide its installed app identity.")
        }
        let app = URL(fileURLWithPath: arguments[2], isDirectory: true).resolvingSymlinksInPath()
        let expected = app.appendingPathComponent("Contents/Helpers/Stainless Updater.app")
        guard app.lastPathComponent == "Stainless.app",
              helperBundle.resolvingSymlinksInPath() == expected.resolvingSymlinksInPath(),
              expected.resolvingSymlinksInPath().path.hasPrefix(app.path + "/") else {
            throw profileError("The native helper does not belong to this installed Stainless.app.")
        }
        return app
    }

    static func installed(in app: URL) throws -> Self {
        let app = app.resolvingSymlinksInPath()
        let info = try manifest(app.appendingPathComponent("Contents/Info.plist"))
        guard app.lastPathComponent == "Stainless.app",
              info["CFBundleIdentifier"] as? String == "dev.stainless.browser",
              info["CFBundleExecutable"] as? String == "Stainless",
              info["CrProductDirName"] as? String == "Stainless",
              info["StainlessManagedModesVersion"] as? NSNumber == 1 else {
            throw profileError("This app does not support managed modes version 1. Install a compatible browser; no default-profile fallback is used.")
        }
        let helper = app.appendingPathComponent("Contents/Helpers/Stainless Updater.app")
        let helperInfo = try manifest(helper.appendingPathComponent("Contents/Info.plist"))
        guard helperInfo["CFBundleIdentifier"] as? String == "dev.stainless.browser.updater",
              helperInfo["StainlessProfileChooserVersion"] as? NSNumber == 1 else {
            throw profileError("This app does not include native profile chooser version 1. Repair or update the installed app.")
        }
        let executable = helper.appendingPathComponent("Contents/Resources/stainless-update")
            .resolvingSymlinksInPath()
        guard executable.path.hasPrefix(app.path + "/"),
              FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw profileError("The bundled profile helper is missing, not executable, or outside this app. No developer-tool fallback is used.")
        }
        return Self(app: app, executable: executable)
    }

    private static func manifest(_ url: URL) throws -> [String: Any] {
        do {
            guard let value = try PropertyListSerialization.propertyList(
                from: Data(contentsOf: url), format: nil) as? [String: Any] else {
                throw profileError("Invalid app manifest at \(url.path).")
            }
            return value
        } catch {
            throw profileError("Cannot read the installed app manifest at \(url.path): \(error.localizedDescription)")
        }
    }

    func metadata(label: String? = nil) throws -> ManagedProfileMetadata {
        struct Request: Encodable {
            let protocol_version = 1
            let request: [String: String]
        }
        let request = Request(request: label.map { ["operation": "create", "label": $0] }
            ?? ["operation": "list"])
        let reply = try exchange("profile-metadata", input: JSONEncoder().encode(request))
        guard let metadata = reply.metadata else {
            throw profileError("The bundled profile helper omitted its metadata result.")
        }
        return try metadata.validated()
    }

    func launch(
        profile: ManagedProfile, mode: ManagedProfileMode,
        started: @escaping @Sendable () -> Void
    ) throws {
        struct Request: Encodable {
            let protocol_version = 1
            let profile_id: String
            let mode: ManagedProfileMode
        }
        _ = try exchange("profile-launch",
            input: JSONEncoder().encode(Request(profile_id: profile.id, mode: mode)),
            started: started)
    }

    private func exchange(
        _ operation: String, input: Data, started: (@Sendable () -> Void)? = nil
    ) throws -> ProfileReply {
        let process = Process()
        process.executableURL = executable
        process.arguments = [operation, "--app", app.path]
        let output = Pipe()
        let incoming = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.standardError
        process.standardInput = incoming
        try process.run()
        // Never terminate a foreground owner or abandon its wait after an I/O/decoding error.
        defer { process.waitUntilExit() }
        var closedInput = false
        defer {
            if !closedInput {
                do {
                    try incoming.fileHandleForWriting.close()
                } catch {
                    FileHandle.standardError.write(Data("Cannot close native profile input: \(error.localizedDescription)\n".utf8))
                }
            }
        }
        try incoming.fileHandleForWriting.write(contentsOf: input)
        try incoming.fileHandleForWriting.close()
        closedInput = true
        var reader = ProfileReplyReader(output.fileHandleForReading)
        var didStart = false
        var terminal: ProfileReply?
        while let reply = try reader.next() {
            guard terminal == nil else {
                throw profileError("The bundled profile helper returned multiple terminal results.")
            }
            switch reply.state {
            case "started" where operation == "profile-launch" && !didStart:
                didStart = true
                started?()
            case "exited" where operation == "profile-launch" && didStart:
                terminal = reply
            case "metadata" where operation == "profile-metadata":
                terminal = reply
            case "error":
                terminal = reply
            default:
                throw profileError("The bundled profile helper returned an unexpected lifecycle result.")
            }
        }
        process.waitUntilExit()
        guard let terminal else {
            throw profileError("The profile helper exited without a result. Check the app installation and available disk space.")
        }
        if terminal.state == "error" { throw profileError(terminal.message) }
        guard process.terminationReason == .exit, process.terminationStatus == 0 else {
            throw profileError("The profile helper exited unexpectedly. Close its browser before retrying or repairing the app.")
        }
        return terminal
    }
}

@MainActor
final class ManagedProfilesWindow: NSWindowController {
    let picker = NSPopUpButton(frame: .zero, pullsDown: false)
    let nameField = NSTextField()
    let createButton = NSButton(title: "Create Profile", target: nil, action: nil)
    let standardButton = NSButton(title: "Open Standard", target: nil, action: nil)
    let incognitoButton = NSButton(title: "Open Incognito", target: nil, action: nil)
    let refreshButton = NSButton(title: "Refresh", target: nil, action: nil)
    let closeButton = NSButton(title: "Close", target: nil, action: nil)
    let statusLabel = NSTextView()
    let updateLabel = NSTextField(wrappingLabelWithString: "")
    private let app: URL
    private var profiles: [ManagedProfile] = []
    private var loading = false
    private var starting: Set<UUID> = []
    private var sessions: Set<UUID> = []
    private(set) var failureMessage: String?
    var statusChanged: (() -> Void)?

    var hasWork: Bool { loading || !starting.isEmpty || !sessions.isEmpty }
    var keepsHelperAlive: Bool { hasWork || failureMessage != nil || window?.isVisible == true }
    var mutationBlockReason: String? {
        didSet {
            updateLabel.stringValue = mutationBlockReason ?? ""
            updateControls()
        }
    }

    init(app: URL) {
        self.app = app
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 580, height: 490),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Managed Profiles"
        window.isReleasedWhenClosed = false
        super.init(window: window)
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        let introduction = NSTextField(wrappingLabelWithString:
            "Choose a separate local profile. No account or network is needed. Your current browser stays open; nothing is imported.")
        let profileLabel = NSTextField(labelWithString: "Managed profile")
        picker.setAccessibilityLabel("Managed profile")
        nameField.placeholderString = "New local profile name"
        nameField.setAccessibilityLabel("New local profile name")
        nameField.target = self
        nameField.action = #selector(createProfile)
        createButton.target = self
        createButton.action = #selector(createProfile)
        let creation = NSStackView(views: [nameField, createButton])
        creation.spacing = 8
        let explanation = NSTextField(wrappingLabelWithString:
            "Standard uses this profile's own local data. Incognito starts fresh with no copied identity or history; downloads persist. Separate roots are not same-user OS isolation. A crash can leave private-session files.")
        explanation.font = .systemFont(ofSize: 12)
        explanation.textColor = .secondaryLabelColor
        standardButton.target = self
        standardButton.action = #selector(openStandard)
        standardButton.keyEquivalent = "\r"
        incognitoButton.target = self
        incognitoButton.action = #selector(openIncognito)
        incognitoButton.toolTip = "Start a new temporary root without copying cookies, passwords or preferences."
        refreshButton.target = self
        refreshButton.action = #selector(refreshProfiles)
        closeButton.target = self
        closeButton.action = #selector(closeChooser)
        closeButton.keyEquivalent = "\u{1b}"
        let actions = NSStackView(views: [standardButton, incognitoButton, refreshButton, closeButton])
        actions.spacing = 8
        let statusScroll = NSScrollView()
        statusScroll.hasVerticalScroller = true
        statusScroll.drawsBackground = false
        statusScroll.documentView = statusLabel
        statusScroll.heightAnchor.constraint(equalToConstant: 90).isActive = true
        statusLabel.isEditable = false
        statusLabel.isSelectable = true
        statusLabel.drawsBackground = false
        statusLabel.font = .systemFont(ofSize: 12)
        statusLabel.isVerticallyResizable = true
        statusLabel.isHorizontallyResizable = false
        statusLabel.minSize = NSSize(width: 0, height: 90)
        statusLabel.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude)
        statusLabel.autoresizingMask = [.width]
        statusLabel.textContainer?.widthTracksTextView = true
        statusLabel.textContainer?.containerSize.height = CGFloat.greatestFiniteMagnitude
        statusLabel.setAccessibilityLabel("Profile status")
        updateLabel.font = .systemFont(ofSize: 12)
        updateLabel.textColor = .secondaryLabelColor
        for view in [introduction, profileLabel, picker, creation, explanation, actions, updateLabel, statusScroll] {
            stack.addArrangedSubview(view)
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        let content = NSView(frame: window.contentLayoutRect)
        window.contentView = content
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -20),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 20),
            stack.bottomAnchor.constraint(lessThanOrEqualTo: content.bottomAnchor, constant: -20),
            nameField.widthAnchor.constraint(greaterThanOrEqualToConstant: 280),
        ])
        let order: [NSView] = [picker, nameField, createButton, standardButton,
            incognitoButton, refreshButton, statusLabel, closeButton]
        for (index, view) in order.enumerated() {
            view.nextKeyView = order[(index + 1) % order.count]
        }
        window.initialFirstResponder = picker
        applyProfiles([], selecting: nil)
    }

    required init?(coder: NSCoder) { nil }

    func present() {
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        if failureMessage == nil { refreshProfiles() }
    }

    func applyProfiles(_ profiles: [ManagedProfile], selecting id: String?) {
        self.profiles = profiles
        picker.removeAllItems()
        if profiles.isEmpty {
            picker.addItem(withTitle: "No managed profiles. Create one below.")
            window?.initialFirstResponder = nameField
        } else {
            for profile in profiles {
                picker.addItem(withTitle: "\(profile.label) (\(profile.id))")
            }
            if let index = profiles.firstIndex(where: { $0.id == id }) {
                picker.selectItem(at: index)
            }
            window?.initialFirstResponder = picker
        }
        updateControls()
    }

    func report(_ message: String, error: Bool = false) {
        if error {
            failureMessage = message
            NSLog("Stainless profiles: %@", message)
        }
        if let failureMessage, !error {
            statusLabel.string = "\(failureMessage)\n\n\(message)"
        } else {
            statusLabel.string = message
        }
        statusLabel.textColor = failureMessage == nil ? .labelColor : .systemRed
        if let window, window.isVisible {
            NSAccessibility.post(element: window, notification: .announcementRequested,
                userInfo: [.announcement: message, .priority: NSAccessibilityPriorityLevel.high.rawValue])
        }
        statusChanged?()
    }

    private var selectedProfile: ManagedProfile? {
        let index = picker.indexOfSelectedItem
        return profiles.indices.contains(index) ? profiles[index] : nil
    }

    private func updateControls() {
        let idle = !loading && starting.isEmpty
        picker.isEnabled = idle && !profiles.isEmpty
        nameField.isEnabled = idle && mutationBlockReason == nil
        createButton.isEnabled = idle && mutationBlockReason == nil
        standardButton.isEnabled = idle && selectedProfile != nil && mutationBlockReason == nil
        incognitoButton.isEnabled = standardButton.isEnabled
        refreshButton.isEnabled = idle
    }

    @objc func refreshProfiles() { loadProfiles() }

    @objc private func createProfile() {
        guard mutationBlockReason == nil else { return }
        let label = nameField.stringValue
        guard !label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              label.utf8.count <= 128, label.rangeOfCharacter(from: .controlCharacters) == nil else {
            report("Enter a profile name with 1-128 bytes of printable text.", error: true)
            window?.makeFirstResponder(nameField)
            return
        }
        loadProfiles(creating: label)
    }

    private func loadProfiles(creating label: String? = nil) {
        guard !loading && starting.isEmpty else { return }
        let selected = selectedProfile?.id
        let app = app
        failureMessage = nil
        loading = true
        report(label == nil ? "Reading local profile metadata..." : "Creating an empty local profile...")
        updateControls()
        Task {
            do {
                let result = try await Task.detached(priority: .userInitiated) {
                    let runtime = try ManagedProfilesRuntime.installed(in: app)
                    return try runtime.metadata(label: label)
                }.value
                if label != nil {
                    guard result.profiles.count == 1, let created = result.profiles.first,
                          !profiles.contains(where: { $0.id == created.id }) else {
                        throw profileError("The helper returned an invalid create result. Refresh the registry before retrying.")
                    }
                    applyProfiles(profiles + result.profiles, selecting: created.id)
                    nameField.stringValue = ""
                } else {
                    applyProfiles(result.profiles, selecting: selected)
                }
                report(label == nil
                    ? "\(result.profiles.count) local profiles. \(sessions.count) managed sessions running."
                    : "Profile created. Choose Standard or Incognito to open it.")
            } catch {
                report(error.localizedDescription, error: true)
            }
            loading = false
            updateControls()
        }
    }

    @objc private func openStandard() { openProfile(.standard) }
    @objc private func openIncognito() { openProfile(.incognito) }
    @objc private func closeChooser() { close() }

    private func openProfile(_ mode: ManagedProfileMode) {
        guard !loading && starting.isEmpty, mutationBlockReason == nil, let profile = selectedProfile else { return }
        let session = UUID()
        let app = app
        failureMessage = nil
        sessions.insert(session)
        starting.insert(session)
        report("Opening \(profile.label) in \(mode.rawValue)...")
        updateControls()
        Task { [self] in
            do {
                try await Task.detached(priority: .userInitiated) {
                    let runtime = try ManagedProfilesRuntime.installed(in: app)
                    try runtime.launch(profile: profile, mode: mode) { [weak self] in
                        Task { @MainActor in
                            guard let self, self.sessions.contains(session) else { return }
                            self.starting.remove(session)
                            self.report("\(profile.label): \(mode.rawValue) process started. Browser health is not yet verified.")
                            self.updateControls()
                        }
                    }
                }.value
                report("\(profile.label): \(mode.rawValue) browser closed. \(sessions.count - 1) managed sessions remain.")
            } catch {
                report(error.localizedDescription, error: true)
            }
            starting.remove(session)
            sessions.remove(session)
            updateControls()
        }
    }
}
