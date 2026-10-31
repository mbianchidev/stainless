import AppKit
import Foundation

private struct Reply: Decodable, Sendable {
    let state: String
    let message: String
    let version: String?
}

private struct Runner: Sendable {
    let executable: URL
    let app: URL

    func run(_ operation: String, observedStart: Bool = false, input: String? = nil) throws -> Reply {
        let process = Process()
        process.executableURL = executable
        process.arguments = [operation, "--app", app.path]
        if observedStart { process.arguments?.append("--observed-start") }
        let output = Pipe()
        let errors = Pipe()
        process.standardOutput = output
        process.standardError = errors
        let incoming = Pipe()
        process.standardInput = incoming
        try process.run()
        if let input { try incoming.fileHandleForWriting.write(contentsOf: Data(input.utf8)) }
        try incoming.fileHandleForWriting.close()
        let bytes = output.fileHandleForReading.readDataToEndOfFile()
        let diagnostics = errors.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let reply: Reply
        do {
            reply = try JSONDecoder().decode(Reply.self, from: bytes)
        } catch {
            throw NSError(domain: "StainlessUpdater", code: Int(process.terminationStatus),
                userInfo: [NSLocalizedDescriptionKey: "Updater returned invalid output. Check its installation and available disk space."])
        }
        if process.terminationStatus != 0 && reply.state != "error" {
            throw NSError(domain: "StainlessUpdater", code: Int(process.terminationStatus),
                userInfo: [NSLocalizedDescriptionKey: "Updater exited unexpectedly."])
        }
        if !diagnostics.isEmpty {
            FileHandle.standardError.write(diagnostics)
        }
        return reply
    }
}

@MainActor
private final class UpdaterDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private let statusLine = NSMenuItem(title: "Starting updates...", action: nil, keyEquivalent: "")
    private let checkItem = NSMenuItem(title: "Check for Updates", action: #selector(checkManually), keyEquivalent: "")
    private let authorizationItem = NSMenuItem(title: "Authorize Private Updates...", action: #selector(authorize), keyEquivalent: "")
    private let restartItem = NSMenuItem(title: "Restart to Update", action: #selector(restart), keyEquivalent: "")
    private let forgetItem = NSMenuItem(title: "Forget Update Authorization", action: #selector(forget), keyEquivalent: "")
    private var runner: Runner?
    private var busy = false
    private var staged = false
    private var stagedVersion: String?
    private var installationFailure: String?
    private var relaunchOnRecovery = false
    private var pendingHealth = false
    private var observedNewLaunch = false
    private var restartRequested = false
    private var sawBrowser = false
    private var lastCheck = Date.distantPast
    private var restartDeadline: Date?
    private var launchObserver: NSObjectProtocol?

    private var browsers: [NSRunningApplication] {
        NSRunningApplication.runningApplications(withBundleIdentifier: "dev.stainless.browser")
            .filter { !$0.isTerminated }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let otherHelpers = NSRunningApplication.runningApplications(
            withBundleIdentifier: "dev.stainless.browser.updater"
        ).filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier && !$0.isTerminated }
        if !otherHelpers.isEmpty {
            NSApp.terminate(nil)
            return
        }
        NSApp.setActivationPolicy(.accessory)
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.image = NSImage(systemSymbolName: "arrow.down.circle", accessibilityDescription: "Stainless Updates")
        statusItem.button?.toolTip = "Stainless preview updates"
        let menu = NSMenu()
        statusLine.isEnabled = false
        menu.addItem(statusLine)
        menu.addItem(.separator())
        for item in [checkItem, authorizationItem, restartItem, forgetItem] {
            item.target = self
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "Quit Updater", action: #selector(quitUpdater), keyEquivalent: "")
        quit.target = self
        menu.addItem(quit)
        statusItem.menu = menu
        updateControls()
        do {
            runner = try prepareRunner()
        } catch {
            report(error.localizedDescription)
            return
        }
        sawBrowser = !browsers.isEmpty
        launchObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didLaunchApplicationNotification, object: nil, queue: .main
        ) { [weak self] notification in
            let isBrowser = (notification.userInfo?[NSWorkspace.applicationUserInfoKey]
                as? NSRunningApplication)?.bundleIdentifier == "dev.stainless.browser"
            if isBrowser {
                Task { @MainActor in
                    guard let self else { return }
                    self.sawBrowser = true
                    if self.pendingHealth { self.observedNewLaunch = true }
                }
            }
        }
        perform("status") { reply in
            self.staged = reply.state == "staged"
            self.pendingHealth = ["awaiting_launch", "awaiting_health"].contains(reply.state)
            self.perform("reconcile", observedStart: !self.browsers.isEmpty) { recovery in
                if !["awaiting_launch", "awaiting_health", "rolled_back"].contains(recovery.state) {
                    self.checkAutomatically()
                }
            }
        }
        Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
    }

    private func prepareRunner() throws -> Runner {
        guard let index = CommandLine.arguments.firstIndex(of: "--app"),
              CommandLine.arguments.indices.contains(index + 1) else {
            throw NSError(domain: "StainlessUpdater", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Browser did not provide its installed app path."])
        }
        let app = URL(fileURLWithPath: CommandLine.arguments[index + 1], isDirectory: true)
        guard let bundle = Bundle(url: app),
              let build = bundle.object(forInfoDictionaryKey: "StainlessBuild") as? NSNumber else {
            throw NSError(domain: "StainlessUpdater", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Installed browser has no build identity."])
        }
        let state = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Stainless Updater/runners", isDirectory: true)
        try FileManager.default.createDirectory(at: state, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        let source = Bundle.main.bundleURL.appendingPathComponent("Contents/Resources/stainless-update")
        let destination = state.appendingPathComponent("stainless-update-\(build.uint64Value)")
        if !FileManager.default.fileExists(atPath: destination.path) {
            let temporary = state.appendingPathComponent("incoming-\(UUID().uuidString)")
            try FileManager.default.copyItem(at: source, to: temporary)
            try FileManager.default.moveItem(at: temporary, to: destination)
        } else if try Data(contentsOf: destination) != Data(contentsOf: source) {
            throw NSError(domain: "StainlessUpdater", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Cached updater differs from this build. Remove that build's runner cache before retrying."])
        }
        return Runner(executable: destination, app: app)
    }

    private func perform(
        _ operation: String, observedStart: Bool = false, input: String? = nil,
        completion: (@MainActor (Reply) -> Void)? = nil
    ) {
        guard !busy, let runner else { return }
        busy = true
        updateControls()
        Task {
            do {
                let reply = try await Task.detached(priority: .utility) {
                    try runner.run(operation, observedStart: observedStart, input: input)
                }.value
                busy = false
                apply(reply)
                completion?(reply)
            } catch {
                busy = false
                report(error.localizedDescription)
            }
            updateControls()
        }
    }

    private func apply(_ reply: Reply) {
        statusItem.button?.image = NSImage(systemSymbolName: "arrow.down.circle", accessibilityDescription: "Stainless Updates")
        statusLine.title = reply.version.map { "\(reply.state): \($0)" } ?? reply.state
        statusItem.button?.toolTip = reply.message
        switch reply.state {
        case "staged":
            if stagedVersion != reply.version { installationFailure = nil }
            staged = true
            stagedVersion = reply.version
        case "awaiting_launch", "awaiting_health":
            staged = false
            pendingHealth = true
        case "idle":
            pendingHealth = false
        case "healthy", "rolled_back":
            pendingHealth = false
            observedNewLaunch = false
            if reply.state == "rolled_back" {
                staged = false
                installationFailure = installationFailure ?? reply.message
            } else if let version = reply.version, version == stagedVersion {
                installationFailure = nil
                stagedVersion = nil
                relaunchOnRecovery = false
            }
        case "error":
            report(reply.message)
        default:
            break
        }
        if let installationFailure, reply.state != "error" {
            report(installationFailure)
        }
        updateControls()
    }

    private func report(_ message: String) {
        statusLine.title = "Update error (hover for details)"
        statusItem.button?.toolTip = message
        statusItem.button?.image = NSImage(systemSymbolName: "exclamationmark.triangle", accessibilityDescription: "Stainless update error")
        NSLog("Stainless updates: %@", message)
    }

    private func updateControls() {
        checkItem.isEnabled = !busy && runner != nil && !pendingHealth
        authorizationItem.isEnabled = !busy && runner != nil
        restartItem.isEnabled = !busy && staged && !restartRequested
        forgetItem.isEnabled = !busy && runner != nil
    }

    private func checkAutomatically() {
        guard !busy && !pendingHealth else { return }
        lastCheck = Date()
        statusLine.title = "Checking/downloading preview..."
        perform("check")
    }

    @objc private func checkManually() { checkAutomatically() }

    @objc private func authorize() {
        let alert = NSAlert()
        alert.messageText = "Private preview updates"
        alert.informativeText = "Use a fine-grained GitHub token with read-only Contents access to mbianchidev/stainless. It stays in your macOS Keychain. No account is needed for browsing."
        let field = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 360, height: 24))
        field.placeholderString = "Read-only GitHub token"
        alert.accessoryView = field
        alert.addButton(withTitle: "Save in Keychain")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let value = field.stringValue
        field.stringValue = ""
        perform("authorize", input: value) { reply in
            if reply.state == "authorized" { self.checkAutomatically() }
        }
    }

    @objc private func forget() { perform("forget") }

    @objc private func restart() {
        guard staged && !busy else { return }
        installationFailure = nil
        relaunchOnRecovery = true
        restartRequested = true
        restartDeadline = Date().addingTimeInterval(60)
        for browser in browsers {
            if !browser.terminate() {
                report("Stainless could not quit. Finish or cancel pending work, then restart to update.")
                restartRequested = false
                relaunchOnRecovery = false
            }
        }
        updateControls()
    }

    private func tick() {
        guard !busy else { return }
        let running = browsers
        if !running.isEmpty { sawBrowser = true }
        if restartRequested, let deadline = restartDeadline, Date() > deadline, !running.isEmpty {
            restartRequested = false
            relaunchOnRecovery = false
            report("Restart canceled: Stainless is still running. Resolve any download or unsaved-work prompt, then retry.")
            updateControls()
        }
        if pendingHealth {
            observedNewLaunch = observedNewLaunch || !running.isEmpty
            perform("reconcile", observedStart: observedNewLaunch) { reply in
                if reply.state == "error" {
                    self.pendingHealth = false
                    self.installationFailure = reply.message
                    self.relaunchOnRecovery = false
                    return
                }
                if reply.state == "rolled_back" && self.relaunchOnRecovery {
                    self.relaunchOnRecovery = false
                    self.launchBrowser()
                }
                if self.installationFailure != nil
                    && ["idle", "healthy", "rolled_back"].contains(reply.state) {
                    self.perform("status")
                }
            }
        } else if staged && running.isEmpty && sawBrowser && installationFailure == nil {
            perform("install") { reply in
                let requestedRestart = self.restartRequested
                guard reply.state == "awaiting_launch" else {
                    self.installationFailure = reply.message
                    self.pendingHealth = true
                    self.restartRequested = false
                    if requestedRestart { self.launchBrowser() }
                    return
                }
                if requestedRestart { self.launchBrowser() }
                self.restartRequested = false
            }
        } else if running.isEmpty && sawBrowser && !staged && installationFailure == nil {
            NSApp.terminate(nil)
        } else if Date().timeIntervalSince(lastCheck) >= 4 * 60 * 60 {
            checkAutomatically()
        }
    }

    private func launchBrowser() {
        guard let runner else { return }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.arguments = ["--restore-last-session"]
        observedNewLaunch = true
        NSWorkspace.shared.openApplication(at: runner.app, configuration: configuration) { _, error in
            if let error {
                let message = error.localizedDescription
                Task { @MainActor in self.report("Browser restart failed: \(message)") }
            }
        }
    }

    @objc private func quitUpdater() {
        guard !busy else {
            report("An update operation is still running; wait for it to finish before quitting the updater.")
            return
        }
        NSApp.terminate(nil)
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let launchObserver {
            NSWorkspace.shared.notificationCenter.removeObserver(launchObserver)
        }
    }
}

let application = NSApplication.shared
private let delegate = UpdaterDelegate()
application.delegate = delegate
application.run()
