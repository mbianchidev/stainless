import AppKit
import Darwin
import Foundation

struct ManagedProfile: Decodable, Equatable, Sendable {
    let id: String
    let label: String
    let profileSchema: UInt32
    let lastBrowser: ManagedProfileBrowser?

    enum CodingKeys: String, CodingKey {
        case id, label
        case profileSchema = "profile_schema"
        case lastBrowser = "last_browser"
    }
}

struct ManagedProfileBrowser: Decodable, Equatable, Sendable {
    struct Product: Decodable, Equatable, Sendable {
        let version: String
        let build: UInt64
        let channel: String
    }

    let product: Product
    let chromiumVersion: String

    enum CodingKeys: String, CodingKey {
        case product
        case chromiumVersion = "chromium_version"
    }

    var summary: String {
        "\(product.version), build \(product.build), \(product.channel); Chromium \(chromiumVersion)"
    }
}

private func profileIdentifierIsValid(_ value: String, prefix: String) -> Bool {
    value.utf8.count == 18 && value.hasPrefix(prefix)
        && value.utf8.dropFirst(2).allSatisfy {
            (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
        }
}

private func profileNameIsValid(_ label: String) -> Bool {
    !label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        && label.utf8.count <= 128 && label.rangeOfCharacter(from: .controlCharacters) == nil
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
                  profileIdentifierIsValid($0.id, prefix: "p-")
                      && $0.profileSchema == 1 && profileNameIsValid($0.label)
              }) else {
            throw profileError("Unsupported or invalid profile metadata. Keep the registry and use a compatible browser.")
        }
        return self
    }
}

struct ManagedProfileBackup: Decodable, Sendable {
    let id: String
    let profile: ManagedProfile?
    let modifiedUnixSeconds: UInt64?
    let compatible: Bool
    let message: String

    enum CodingKeys: String, CodingKey {
        case id, profile, compatible, message
        case modifiedUnixSeconds = "modified_unix_seconds"
    }
}

struct ManagedBackupCatalog: Decodable, Sendable {
    let backups: [ManagedProfileBackup]
    let nextAfter: String?

    enum CodingKeys: String, CodingKey {
        case backups
        case nextAfter = "next_after"
    }

    func validated() throws -> Self {
        guard backups.count <= 64, Set(backups.map(\.id)).count == backups.count,
              backups.allSatisfy({
                  profileIdentifierIsValid($0.id, prefix: "b-") && !$0.message.isEmpty
                      && $0.message.utf8.count <= 4096
                      && (!$0.compatible || ($0.profile != nil && $0.modifiedUnixSeconds != nil))
              }),
              nextAfter == nil || (nextAfter == backups.last?.id
                  && profileIdentifierIsValid(nextAfter ?? "", prefix: "b-")) else {
            throw profileError("Unsupported or invalid backup metadata. Keep all backups and use a compatible helper.")
        }
        for backup in backups {
            if let profile = backup.profile {
                _ = try ManagedProfileMetadata(protocolVersion: 1, registryVersion: 1,
                    profiles: [profile]).validated()
            }
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
    let catalog: ManagedBackupCatalog?
    let backupID: String?
    let safeMode: Bool?

    enum CodingKeys: String, CodingKey {
        case state, message, metadata, catalog
        case protocolVersion = "protocol_version"
        case backupID = "backup_id"
        case safeMode = "safe_mode"
    }
}

private struct ProfileRecoveryRequest<Payload: Encodable>: Encodable {
    let protocol_version = 1
    let request: Payload
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
            var chunk = Data(count: 4096)
            // A bounded FileHandle read can wait for EOF instead of delivering a live handshake.
            let count = chunk.withUnsafeMutableBytes {
                Darwin.read(handle.fileDescriptor, $0.baseAddress, $0.count)
            }
            if count < 0 {
                let code = errno
                if code == EINTR { continue }
                throw NSError(domain: NSPOSIXErrorDomain, code: Int(code),
                    userInfo: [NSLocalizedDescriptionKey:
                        "Cannot read native profile reply: \(String(cString: strerror(code)))"])
            }
            guard count > 0 else {
                guard bytes.isEmpty else {
                    throw profileError("The bundled profile helper returned an incomplete result.")
                }
                return nil
            }
            chunk.count = count
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
        let reply = try exchange("profile-metadata", input: JSONEncoder().encode(request),
            terminalState: "metadata")
        guard let metadata = reply.metadata else {
            throw profileError("The bundled profile helper omitted its metadata result.")
        }
        return try metadata.validated()
    }

    func launch(
        profile: ManagedProfile, mode: ManagedProfileMode,
        safeMode: Bool = false,
        started: @escaping @Sendable () -> Void
    ) throws {
        if safeMode {
            guard mode == .standard else {
                throw profileError("Startup Safe Mode is only available for a managed Standard profile.")
            }
            try requireSafeMode()
        }
        struct Request: Encodable {
            let protocol_version = 1
            let profile_id: String
            let mode: ManagedProfileMode
            let safe_mode: Bool?
        }
        _ = try exchange("profile-launch",
            input: JSONEncoder().encode(Request(
                profile_id: profile.id, mode: mode, safe_mode: safeMode ? true : nil)),
            terminalState: "exited",
            started: started, safeMode: safeMode)
    }

    private func requireSafeMode() throws {
        let current = try Self.installed(in: app)
        let browser = try Self.manifest(app.appendingPathComponent("Contents/Info.plist"))
        let helper = try Self.manifest(app.appendingPathComponent(
            "Contents/Helpers/Stainless Updater.app/Contents/Info.plist"))
        guard current.executable == executable,
              browser["StainlessManagedSafeModeVersion"] as? NSNumber == 1,
              helper["StainlessProfileSafeModeVersion"] as? NSNumber == 1 else {
            throw profileError("This browser/helper pair does not support startup Safe Mode version 1. Install a compatible app; no normal-launch fallback is used.")
        }
    }

    private func requireRecovery() throws {
        let current = try Self.installed(in: app)
        let info = try Self.manifest(app.appendingPathComponent(
            "Contents/Helpers/Stainless Updater.app/Contents/Info.plist"))
        guard current.executable == executable,
              info["StainlessProfileRecoveryVersion"] as? NSNumber == 1 else {
            throw profileError("This app does not include native profile recovery version 1. Repair or update the installed app.")
        }
    }

    func backups(after: String? = nil) throws -> ManagedBackupCatalog {
        try requireRecovery()
        struct Payload: Encodable {
            let operation = "list"
            let after: String?
        }
        let reply = try exchange("profile-recovery",
            input: JSONEncoder().encode(ProfileRecoveryRequest(request: Payload(after: after))),
            terminalState: "backups")
        guard let catalog = reply.catalog else {
            throw profileError("The bundled helper omitted backup metadata. No backup has been selected.")
        }
        return try catalog.validated()
    }

    func backup(profile: ManagedProfile, confirmed: Bool) throws -> String {
        try requireRecovery()
        struct Payload: Encodable {
            let operation = "backup"
            let profile_id: String
            let confirmed: Bool
        }
        let reply = try exchange("profile-recovery",
            input: JSONEncoder().encode(ProfileRecoveryRequest(
                request: Payload(profile_id: profile.id, confirmed: confirmed))),
            terminalState: "backed_up")
        guard let id = reply.backupID, profileIdentifierIsValid(id, prefix: "b-") else {
            throw profileError("The helper returned an invalid backup result. A commit may already exist; refresh backups before retrying.")
        }
        return id
    }

    func restore(backup: String, label: String, confirmed: Bool) throws -> ManagedProfile {
        try requireRecovery()
        struct Payload: Encodable {
            let operation = "restore"
            let backup_id: String
            let label: String
            let confirmed: Bool
        }
        let reply = try exchange("profile-recovery",
            input: JSONEncoder().encode(ProfileRecoveryRequest(
                request: Payload(backup_id: backup, label: label, confirmed: confirmed))),
            terminalState: "restored")
        guard let metadata = try reply.metadata?.validated(), metadata.profiles.count == 1,
              let profile = metadata.profiles.first, profile.label == label else {
            throw profileError("The helper returned an invalid restore result. A new identity may already be registered; refresh profiles before retrying.")
        }
        return profile
    }

    private func exchange(
        _ operation: String, input: Data, terminalState: String,
        started: (@Sendable () -> Void)? = nil, safeMode: Bool = false
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
        defer {
            do {
                try output.fileHandleForReading.close()
            } catch {
                FileHandle.standardError.write(Data("Cannot close native profile output: \(error.localizedDescription)\n".utf8))
            }
            process.waitUntilExit()
        }
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
            if operation == "profile-launch" && ["started", "exited"].contains(reply.state) {
                guard (reply.safeMode ?? false) == safeMode else {
                    throw profileError("The bundled helper did not confirm Safe Mode as requested. Close its browser normally and repair the app before retrying.")
                }
            }
            guard terminal == nil else {
                throw profileError("The bundled profile helper returned multiple terminal results.")
            }
            switch reply.state {
            case "started" where operation == "profile-launch" && !didStart:
                didStart = true
                started?()
            case let state where state == terminalState && (operation != "profile-launch" || didStart):
                terminal = reply
            case "error":
                terminal = reply
            default:
                throw profileError("The bundled profile helper returned an unexpected lifecycle result.")
            }
        }
        process.waitUntilExit()
        guard let terminal else {
            throw profileError("The profile helper exited without a confirmed result. Check the app and disk space; a recovery commit may already exist. Refresh profiles and backups before retrying.")
        }
        if terminal.state == "error" { throw profileError(terminal.message) }
        guard process.terminationReason == .exit, process.terminationStatus == 0 else {
            throw profileError("The profile helper exited unexpectedly. Close its browser before retrying or repairing the app.")
        }
        return terminal
    }
}

@MainActor
private func profileStatusScroll(_ text: NSTextView, height: CGFloat = 90) -> NSScrollView {
    let scroll = NSScrollView()
    scroll.hasVerticalScroller = true
    scroll.drawsBackground = false
    scroll.documentView = text
    scroll.heightAnchor.constraint(equalToConstant: height).isActive = true
    text.isEditable = false
    text.isSelectable = true
    text.drawsBackground = false
    text.font = .systemFont(ofSize: 12)
    text.isVerticallyResizable = true
    text.isHorizontallyResizable = false
    text.minSize = NSSize(width: 0, height: height)
    text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude,
        height: CGFloat.greatestFiniteMagnitude)
    text.autoresizingMask = [.width]
    text.textContainer?.widthTracksTextView = true
    text.textContainer?.containerSize.height = CGFloat.greatestFiniteMagnitude
    return scroll
}

@MainActor
final class ManagedProfilesWindow: NSWindowController {
    let picker = NSPopUpButton(frame: .zero, pullsDown: false)
    let nameField = NSTextField()
    let createButton = NSButton(title: "Create Profile", target: nil, action: nil)
    let standardButton = NSButton(title: "Open Standard", target: nil, action: nil)
    let incognitoButton = NSButton(title: "Open Incognito", target: nil, action: nil)
    let safeModeButton = NSButton(title: "Open Safe Mode", target: nil, action: nil)
    let recoveryButton = NSButton(title: "Backup / Restore...", target: nil, action: nil)
    let refreshButton = NSButton(title: "Refresh", target: nil, action: nil)
    let closeButton = NSButton(title: "Close", target: nil, action: nil)
    let statusLabel = NSTextView()
    let updateLabel = NSTextField(wrappingLabelWithString: "")
    let safeModeLabel = NSTextField(wrappingLabelWithString: "")
    private let app: URL
    private var profiles: [ManagedProfile] = []
    private var loading = false
    private var starting: Set<UUID> = []
    private var sessions: Set<UUID> = []
    private var safeSessions: [UUID: ManagedProfile] = [:]
    private var normalReturnProfiles: Set<String> = []
    private(set) var recoveryWindow: ManagedProfileRecoveryWindow?
    private(set) var failureMessage: String?
    var statusChanged: (() -> Void)?
    var requestBrowserQuit: (() -> Bool)? {
        didSet { recoveryWindow?.requestBrowserQuit = requestBrowserQuit }
    }
    var runningBrowserCount = 1 {
        didSet { recoveryWindow?.runningBrowserCount = runningBrowserCount }
    }
    var recoveryBlockReason: String? {
        didSet { recoveryWindow?.blockReason = recoveryBlockReason }
    }

    var hasWork: Bool {
        loading || !starting.isEmpty || !sessions.isEmpty || recoveryWindow?.hasWork == true
    }
    var holdsRecoveryBoundary: Bool { recoveryWindow?.holdsRecoveryBoundary == true }
    var actionNeeded: String? { failureMessage ?? recoveryWindow?.failureMessage }
    var keepsHelperAlive: Bool {
        hasWork || holdsRecoveryBoundary || actionNeeded != nil
            || !normalReturnProfiles.isEmpty || window?.isVisible == true
    }
    var mutationBlockReason: String? {
        didSet {
            updateLabel.stringValue = mutationBlockReason ?? ""
            updateControls()
        }
    }

    init(app: URL) {
        self.app = app
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 580, height: 660),
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
        picker.target = self
        picker.action = #selector(profileSelectionChanged)
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
        safeModeButton.target = self
        safeModeButton.action = #selector(openSafeMode)
        safeModeButton.toolTip = "Temporarily open this Standard identity without extensions, background apps or previous-session restoration. No settings reset."
        recoveryButton.target = self
        recoveryButton.action = #selector(showRecovery)
        recoveryButton.toolTip = "Pause updates and review cold backups or restore into a new identity."
        refreshButton.target = self
        refreshButton.action = #selector(refreshProfiles)
        closeButton.target = self
        closeButton.action = #selector(closeChooser)
        closeButton.keyEquivalent = "\u{1b}"
        let actions = NSStackView(views: [standardButton, incognitoButton])
        actions.spacing = 8
        let recoveryActions = NSStackView(views: [safeModeButton, recoveryButton])
        recoveryActions.spacing = 8
        let navigation = NSStackView(views: [refreshButton, closeButton])
        navigation.spacing = 8
        let statusScroll = profileStatusScroll(statusLabel)
        statusLabel.setAccessibilityLabel("Profile status")
        updateLabel.font = .systemFont(ofSize: 12)
        updateLabel.textColor = .secondaryLabelColor
        safeModeLabel.font = .systemFont(ofSize: 12)
        safeModeLabel.isSelectable = true
        safeModeLabel.setAccessibilityLabel("Temporary startup restrictions and normal return")
        for view in [introduction, profileLabel, picker, creation, explanation, actions,
            safeModeLabel, recoveryActions, navigation, updateLabel, statusScroll] {
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
            incognitoButton, safeModeButton, recoveryButton, refreshButton, statusLabel, closeButton]
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
        let idle = !loading && starting.isEmpty && recoveryWindow?.hasWork != true
        let mayLaunch = idle && mutationBlockReason == nil && !holdsRecoveryBoundary
        let selected = selectedProfile
        let safeActive = safeSessions.values.contains { $0.id == selected?.id }
        let safeStarting = safeSessions.contains {
            $0.value.id == selected?.id && starting.contains($0.key)
        }
        picker.isEnabled = idle && !profiles.isEmpty
        nameField.isEnabled = mayLaunch
        createButton.isEnabled = mayLaunch
        standardButton.isEnabled = mayLaunch && selected != nil && !safeActive
        incognitoButton.isEnabled = mayLaunch && selected != nil
        safeModeButton.isEnabled = standardButton.isEnabled
        standardButton.title = selected.map { normalReturnProfiles.contains($0.id) } == true
            ? "Open Normally" : "Open Standard"
        let restrictions = "Extensions/background apps and automatic previous-session restoration are off for Safe Mode only. Saved preferences and previous-session data are kept; Safe Mode tabs are not saved for restoration."
        if safeStarting {
            safeModeLabel.stringValue = "Starting Safe Mode for the selected profile. Waiting for the compatible helper's acknowledgement. \(restrictions)"
        } else if safeActive {
            safeModeLabel.stringValue = "Safe Mode active for the selected profile. \(restrictions) Save work and quit its browser normally before opening Standard."
        } else if standardButton.title == "Open Normally" {
            safeModeLabel.stringValue = "Safe Mode is closed. Choose Open Normally to return to this profile's normal startup. \(restrictions)"
        } else if !safeSessions.isEmpty {
            safeModeLabel.stringValue = "Safe Mode active: \(safeSessions.count) other managed sessions. \(restrictions)"
        } else {
            safeModeLabel.stringValue = "If Standard startup repeatedly fails, explicitly try Safe Mode. \(restrictions)"
        }
        recoveryButton.isEnabled = holdsRecoveryBoundary
            || (idle && mutationBlockReason == nil && recoveryBlockReason == nil)
        refreshButton.isEnabled = idle
        recoveryWindow?.hasManagedSessions = !sessions.isEmpty || !starting.isEmpty
    }

    @objc func refreshProfiles() { loadProfiles() }
    @objc func profileSelectionChanged() { updateControls() }

    @objc private func createProfile() {
        guard mutationBlockReason == nil && !holdsRecoveryBoundary else { return }
        let label = nameField.stringValue
        guard profileNameIsValid(label) else {
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
    @objc private func openSafeMode() { openProfile(.standard, safeMode: true) }
    @objc private func closeChooser() { close() }

    @objc func showRecovery() {
        prepareRecovery()?.present()
    }

    func prepareRecovery() -> ManagedProfileRecoveryWindow? {
        guard holdsRecoveryBoundary || (!loading && starting.isEmpty
            && mutationBlockReason == nil && recoveryBlockReason == nil) else { return nil }
        if recoveryWindow == nil {
            let recovery = ManagedProfileRecoveryWindow(app: app)
            recovery.requestBrowserQuit = requestBrowserQuit
            recovery.statusChanged = { [weak self] in
                self?.updateControls()
                self?.statusChanged?()
            }
            recovery.restored = { [weak self] profile in
                guard let self else { return }
                self.applyProfiles(self.profiles + [profile], selecting: profile.id)
                self.report("New recovered identity selected. Finish Recovery, then explicitly open it when ready.")
            }
            recoveryWindow = recovery
        }
        recoveryWindow?.runningBrowserCount = runningBrowserCount
        recoveryWindow?.blockReason = recoveryBlockReason
        updateControls()
        statusChanged?()
        return recoveryWindow
    }

    private func openProfile(_ mode: ManagedProfileMode, safeMode: Bool = false) {
        guard !loading && starting.isEmpty, mutationBlockReason == nil, !holdsRecoveryBoundary,
              let profile = selectedProfile else { return }
        guard mode != .standard || !safeSessions.values.contains(where: { $0.id == profile.id }) else {
            report("Quit this profile's Safe Mode browser normally before opening it again.", error: true)
            return
        }
        let session = UUID()
        let app = app
        let launchName = safeMode ? "Safe Mode" : mode.rawValue
        failureMessage = nil
        sessions.insert(session)
        starting.insert(session)
        if safeMode { safeSessions[session] = profile }
        report("Opening \(profile.label) in \(launchName)...")
        updateControls()
        Task { [self] in
            do {
                try await Task.detached(priority: .userInitiated) {
                    let runtime = try ManagedProfilesRuntime.installed(in: app)
                    try runtime.launch(profile: profile, mode: mode, safeMode: safeMode) { [weak self] in
                        Task { @MainActor in
                            guard let self, self.sessions.contains(session) else { return }
                            self.starting.remove(session)
                            if safeMode { self.normalReturnProfiles.insert(profile.id) }
                            else if mode == .standard { self.normalReturnProfiles.remove(profile.id) }
                            self.report("\(profile.label): \(launchName) process started. This is not confirmation of profile health.")
                            self.updateControls()
                        }
                    }
                }.value
                if safeMode {
                    normalReturnProfiles.insert(profile.id)
                    report("\(profile.label): Safe Mode browser closed. Choose Open Normally to return. Previous-session data are retained; Safe Mode tabs were not saved for restoration.")
                } else {
                    if mode == .standard { normalReturnProfiles.remove(profile.id) }
                    report("\(profile.label): \(mode.rawValue) browser closed. \(sessions.count - 1) managed sessions remain.")
                }
            } catch {
                let recovery = safeMode
                    ? "\n\nSafe Mode did not confirm successful recovery. Saved roots and previous-session data are retained. Close any remaining browser normally, then retry or choose Backup / Restore for a verified backup or a new recovered identity."
                    : ""
                report(error.localizedDescription + recovery, error: true)
            }
            starting.remove(session)
            sessions.remove(session)
            safeSessions.removeValue(forKey: session)
            updateControls()
        }
    }
}

@MainActor
final class ManagedProfileRecoveryWindow: NSWindowController, NSWindowDelegate, NSTextFieldDelegate {
    let profilePicker = NSPopUpButton(frame: .zero, pullsDown: false)
    let backupPicker = NSPopUpButton(frame: .zero, pullsDown: false)
    let backupButton = NSButton(title: "Back Up Selected", target: nil, action: nil)
    let restoreNameField = NSTextField()
    let restoreButton = NSButton(title: "Restore New Profile", target: nil, action: nil)
    let consentButton = NSButton(
        checkboxWithTitle: "I understand these limits and approve my selected operation.",
        target: nil, action: nil)
    let quitBrowsersButton = NSButton(title: "Request Normal Browser Quit", target: nil, action: nil)
    let refreshButton = NSButton(title: "Refresh Profiles / Backups", target: nil, action: nil)
    let moreButton = NSButton(title: "More Backups", target: nil, action: nil)
    let finishButton = NSButton(title: "Finish Recovery", target: nil, action: nil)
    let boundaryLabel = NSTextField(wrappingLabelWithString: "")
    let detailsLabel = NSTextField(wrappingLabelWithString: "Select a backup to review its metadata.")
    let statusLabel = NSTextView()
    private let app: URL
    private var profiles: [ManagedProfile] = []
    private var backups: [ManagedProfileBackup] = []
    private var nextAfter: String?
    private var reviewReady = false
    private(set) var hasWork = false
    private(set) var holdsRecoveryBoundary = true
    private(set) var failureMessage: String?
    var statusChanged: (() -> Void)?
    var restored: ((ManagedProfile) -> Void)?
    var requestBrowserQuit: (() -> Bool)? { didSet { updateControls() } }
    var runningBrowserCount = 1 { didSet { updateControls() } }
    var hasManagedSessions = false { didSet { updateControls() } }
    var blockReason: String? { didSet { updateControls() } }

    init(app: URL) {
        self.app = app
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 740),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Managed Profile Recovery"
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = false
        let introduction = NSTextField(
            wrappingLabelWithString:
                "Recovery pauses this helper's update checks and installation until you finish or close this window. Nothing closes or copies automatically."
        )
        let limits = NSTextField(
            wrappingLabelWithString:
                "Backups are sensitive plaintext, not encrypted or authenticated. Restore copies a verified backup into a NEW managed identity/root; it never overwrites your current or failed profiles. It does not restore an app bundle or move Keychain credentials. Keep your original roots, snapshots and matching app."
        )
        limits.font = .systemFont(ofSize: 12)
        limits.textColor = .secondaryLabelColor
        profilePicker.setAccessibilityLabel("Managed profile to back up")
        backupPicker.setAccessibilityLabel("Backup to restore into a new profile")
        restoreNameField.placeholderString = "New recovered profile name"
        restoreNameField.setAccessibilityLabel("New recovered profile name")
        restoreNameField.delegate = self
        restoreNameField.target = self
        restoreNameField.action = #selector(restoreSelected)
        profilePicker.target = self
        profilePicker.action = #selector(profileSelectionChanged)
        backupPicker.target = self
        backupPicker.action = #selector(backupSelectionChanged)
        for (button, action) in [
            (quitBrowsersButton, #selector(requestQuit)),
            (backupButton, #selector(backUpSelected)),
            (restoreButton, #selector(restoreSelected)),
            (consentButton, #selector(consentChanged)),
            (refreshButton, #selector(refreshRecovery)),
            (moreButton, #selector(loadMore)),
            (finishButton, #selector(finishRecovery)),
        ] {
            button.target = self
            button.action = action
        }
        finishButton.keyEquivalent = "\u{1b}"
        detailsLabel.isSelectable = true
        detailsLabel.font = .systemFont(ofSize: 12)
        detailsLabel.setAccessibilityLabel("Selected backup metadata and compatibility")
        detailsLabel.heightAnchor.constraint(greaterThanOrEqualToConstant: 90).isActive = true
        boundaryLabel.font = .systemFont(ofSize: 12)
        let source = NSStackView(views: [profilePicker, backupButton])
        let snapshots = NSStackView(views: [backupPicker, moreButton])
        let destination = NSStackView(views: [restoreNameField, restoreButton])
        let navigation = NSStackView(views: [refreshButton, finishButton])
        for row in [source, snapshots, destination, navigation] { row.spacing = 8 }
        let status = profileStatusScroll(statusLabel, height: 80)
        statusLabel.setAccessibilityLabel("Recovery status")
        for view in [
            introduction, boundaryLabel, quitBrowsersButton, source, snapshots,
            detailsLabel, destination, limits, consentButton, navigation, status,
        ] {
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
            profilePicker.widthAnchor.constraint(greaterThanOrEqualToConstant: 380),
            backupPicker.widthAnchor.constraint(greaterThanOrEqualToConstant: 380),
            restoreNameField.widthAnchor.constraint(greaterThanOrEqualToConstant: 360),
        ])
        let order: [NSView] = [
            quitBrowsersButton, profilePicker, backupButton, backupPicker,
            moreButton, restoreNameField, restoreButton, consentButton, refreshButton,
            statusLabel, finishButton,
        ]
        for (index, view) in order.enumerated() {
            view.nextKeyView = order[(index + 1) % order.count]
        }
        window.initialFirstResponder = quitBrowsersButton
        applyProfiles([], selecting: nil)
        applyBackups([], selecting: nil)
        updateControls()
    }

    required init?(coder: NSCoder) { nil }

    func present() {
        holdsRecoveryBoundary = true
        statusChanged?()
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        if failureMessage == nil { refreshRecovery() }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !hasWork else {
            report("Wait for the profile operation to finish. It may still be copying or committing data.", error: true)
            return false
        }
        holdsRecoveryBoundary = false
        statusChanged?()
        return true
    }

    private var selectedProfile: ManagedProfile? {
        let index = profilePicker.indexOfSelectedItem - 1
        return profiles.indices.contains(index) ? profiles[index] : nil
    }

    private var selectedBackup: ManagedProfileBackup? {
        let index = backupPicker.indexOfSelectedItem - 1
        return backups.indices.contains(index) ? backups[index] : nil
    }

    private func applyProfiles(_ profiles: [ManagedProfile], selecting id: String?) {
        self.profiles = profiles
        profilePicker.removeAllItems()
        profilePicker.addItem(withTitle: "Select a managed profile to back up")
        for profile in profiles {
            profilePicker.addItem(withTitle: "\(profile.label) (\(profile.id))")
        }
        if let index = profiles.firstIndex(where: { $0.id == id }) {
            profilePicker.selectItem(at: index + 1)
        }
    }

    private func applyBackups(_ backups: [ManagedProfileBackup], selecting id: String?) {
        self.backups = backups
        backupPicker.removeAllItems()
        backupPicker.addItem(withTitle: backups.isEmpty ? "No backups loaded" : "Select a backup to review")
        for backup in backups {
            let label = backup.profile?.label ?? "Unavailable metadata"
            backupPicker.addItem(withTitle: "\(label) | \(backup.id)\(backup.compatible ? "" : " | unavailable")")
        }
        if let index = backups.firstIndex(where: { $0.id == id }) {
            backupPicker.selectItem(at: index + 1)
        }
        updateDetails()
    }

    private func updateDetails() {
        guard let backup = selectedBackup else {
            detailsLabel.stringValue =
                "Select a committed backup. Only metadata is shown; restore verifies the full file inventory before registering a new identity."
            return
        }
        var details = ["Backup: \(backup.id)"]
        if let profile = backup.profile {
            details.append("Source: \(profile.label) (\(profile.id)); schema \(profile.profileSchema)")
            details.append("Browser floor: \(profile.lastBrowser?.summary ?? "Empty profile, never opened")")
        }
        if let modified = backup.modifiedUnixSeconds {
            details.append("Manifest modified: \(Date(timeIntervalSince1970: TimeInterval(modified)).formatted())")
        }
        details.append(backup.message)
        detailsLabel.stringValue = details.joined(separator: "\n")
    }

    private func updateControls() {
        let idle = !hasWork && holdsRecoveryBoundary
        let quiet = runningBrowserCount == 0 && !hasManagedSessions
        boundaryLabel.stringValue =
            blockReason
            ?? (quiet
                ? "No browser is reported running. Each copy still requires the exclusive installation lock and verified ownership."
                : "Save work and quit every Stainless browser normally. Resolve any download or unsaved-work prompt; this window stays open. \(runningBrowserCount) browser apps reported.")
        let approved = idle && quiet && reviewReady && blockReason == nil && consentButton.state == .on
        profilePicker.isEnabled = idle && !profiles.isEmpty
        backupPicker.isEnabled = idle && !backups.isEmpty
        backupButton.isEnabled = approved && selectedProfile != nil
        restoreNameField.isEnabled = idle && blockReason == nil
        restoreButton.isEnabled =
            approved && selectedBackup?.compatible == true
            && profileNameIsValid(restoreNameField.stringValue)
        consentButton.isEnabled = idle && reviewReady && blockReason == nil
        refreshButton.isEnabled = idle
        moreButton.isEnabled = idle && nextAfter != nil
        finishButton.isEnabled = !hasWork
        quitBrowsersButton.isEnabled = idle && !quiet && blockReason == nil && requestBrowserQuit != nil
        window?.initialFirstResponder = quitBrowsersButton.isEnabled ? quitBrowsersButton
            : (profilePicker.isEnabled ? profilePicker : refreshButton)
    }

    private func report(_ message: String, error: Bool = false) {
        if error {
            failureMessage = message
            NSLog("Stainless profile recovery: %@", message)
        }
        statusLabel.string = failureMessage.map { error ? $0 : "\($0)\n\n\(message)" } ?? message
        statusLabel.textColor = failureMessage == nil ? .labelColor : .systemRed
        if let window, window.isVisible {
            NSAccessibility.post(
                element: window, notification: .announcementRequested,
                userInfo: [.announcement: message, .priority: NSAccessibilityPriorityLevel.high.rawValue])
        }
        statusChanged?()
    }

    @objc func profileSelectionChanged() {
        consentButton.state = .off
        updateControls()
    }

    @objc func backupSelectionChanged() {
        consentButton.state = .off
        updateDetails()
        updateControls()
    }

    @objc private func consentChanged() { updateControls() }
    func controlTextDidChange(_ notification: Notification) { updateControls() }

    @objc private func requestQuit() {
        guard quitBrowsersButton.isEnabled, let requestBrowserQuit else { return }
        if requestBrowserQuit() {
            report(
                "Normal quit requested. Resolve browser prompts yourself; copies remain blocked until every browser and supervisor releases ownership."
            )
        } else {
            report(
                "A browser refused the normal quit request. Finish or cancel pending work, quit it normally, then refresh. No process was force-closed.",
                error: true)
        }
    }

    @objc private func finishRecovery() { window?.performClose(nil) }
    @objc func refreshRecovery() { loadRecovery() }
    @objc private func loadMore() {
        guard let nextAfter else { return }
        loadRecovery(after: nextAfter)
    }

    private func loadRecovery(after: String? = nil, selecting id: String? = nil, committed: String? = nil) {
        guard !hasWork && holdsRecoveryBoundary else { return }
        let selectedSource = selectedProfile?.id
        let selectedSnapshot = id ?? selectedBackup?.id
        let app = app
        hasWork = true
        reviewReady = false
        failureMessage = nil
        consentButton.state = .off
        report(committed ?? "Reading local profile and committed-backup metadata. No browsing files are being opened.")
        updateControls()
        Task {
            do {
                let result = try await Task.detached(priority: .userInitiated) {
                    let runtime = try ManagedProfilesRuntime.installed(in: app)
                    let profiles = after == nil ? try runtime.metadata() : nil
                    return (profiles, try runtime.backups(after: after))
                }.value
                if let metadata = result.0 { applyProfiles(metadata.profiles, selecting: selectedSource) }
                let combined = after == nil ? result.1.backups : backups + result.1.backups
                guard Set(combined.map(\.id)).count == combined.count else {
                    throw profileError("Backup metadata changed during pagination. Refresh before selecting a backup.")
                }
                nextAfter = result.1.nextAfter
                applyBackups(combined, selecting: selectedSnapshot)
                reviewReady = true
                report(
                    committed
                        ?? "\(backups.count) backup records loaded. Select a source or backup and explicitly approve an operation; unavailable copies are retained."
                )
            } catch {
                report(
                    [committed, "Metadata refresh failed: \(error.localizedDescription)"]
                        .compactMap { $0 }.joined(separator: "\n\n"), error: true)
            }
            hasWork = false
            updateControls()
        }
    }

    @objc private func backUpSelected() {
        guard backupButton.isEnabled, let profile = selectedProfile else { return }
        let app = app
        hasWork = true
        failureMessage = nil
        consentButton.state = .off
        report(
            "Backing up \(profile.label) (\(profile.id)). Keep this helper running until the verified copy is committed."
        )
        updateControls()
        Task {
            do {
                let id = try await Task.detached(priority: .userInitiated) {
                    try ManagedProfilesRuntime.installed(in: app).backup(profile: profile, confirmed: true)
                }.value
                hasWork = false
                loadRecovery(
                    selecting: id,
                    committed:
                        "Backup \(id) committed. The source profile is unchanged. Review it before any clone-restore.")
            } catch {
                reportRecoveryFailure(error)
                hasWork = false
                updateControls()
            }
        }
    }

    @objc private func restoreSelected() {
        guard restoreButton.isEnabled, let backup = selectedBackup else { return }
        let label = restoreNameField.stringValue
        let app = app
        hasWork = true
        failureMessage = nil
        consentButton.state = .off
        report(
            "Verifying \(backup.id) and restoring a NEW profile named \(label). Existing roots and backup are retained."
        )
        updateControls()
        Task {
            do {
                let profile = try await Task.detached(priority: .userInitiated) {
                    try ManagedProfilesRuntime.installed(in: app).restore(
                        backup: backup.id, label: label, confirmed: true)
                }.value
                guard !profiles.contains(where: { $0.id == profile.id }),
                    profile.id != backup.profile?.id
                else {
                    throw profileError("The helper did not return a new identity. Refresh profiles before retrying.")
                }
                applyProfiles(profiles + [profile], selecting: profile.id)
                restoreNameField.stringValue = ""
                restored?(profile)
                report(
                    "Restored a new profile: \(profile.label) (\(profile.id)). Originals, failed roots and backup remain. No app or Keychain item was restored. Finish Recovery before explicitly opening it."
                )
            } catch {
                reportRecoveryFailure(error)
            }
            hasWork = false
            updateControls()
        }
    }

    private func reportRecoveryFailure(_ error: Error) {
        reviewReady = false
        report(
            "\(error.localizedDescription)\n\nOriginal roots and backups are retained. If completion was interrupted or unconfirmed, a new commit may already exist. Refresh profiles/backups before retrying; do not delete locks or retained copies.",
            error: true)
    }
}
