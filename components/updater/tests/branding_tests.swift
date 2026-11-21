import AppKit

private func expect(_ condition: Bool, _ message: String) throws {
    if !condition {
        throw NSError(domain: "StainlessBrandingTests", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}

@main
@MainActor
struct UpdaterBrandingTests {
    static func main() throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        try expect(CommandLine.arguments.count == 4, "Expected the logo, menu-bar template and offscreen snapshot paths")
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("stainless-branding-\(UUID().uuidString).app", isDirectory: true)
        defer {
            do {
                try FileManager.default.removeItem(at: root)
            } catch {
                FileHandle.standardError.write(Data("Cannot remove branding fixture: \(error.localizedDescription)\n".utf8))
            }
        }
        let resources = root.appendingPathComponent("Contents/Resources", isDirectory: true)
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        let icon = resources.appendingPathComponent("stainless.png")
        try FileManager.default.copyItem(at: URL(fileURLWithPath: CommandLine.arguments[1]), to: icon)
        let statusIcon = resources.appendingPathComponent("stainless-status.png")
        try FileManager.default.copyItem(at: URL(fileURLWithPath: CommandLine.arguments[2]), to: statusIcon)
        try PropertyListSerialization.data(fromPropertyList: [
            "CFBundleIdentifier": "dev.stainless.synthetic-branding",
            "CFBundlePackageType": "APPL",
            "CFBundleIconFile": "stainless.png",
            "StainlessStatusIconFile": "stainless-status.png",
        ], format: .xml, options: 0).write(to: root.appendingPathComponent("Contents/Info.plist"))
        guard let bundle = Bundle(url: root) else {
            throw UpdaterBrandingError.missingLogo
        }
        guard let original = NSImage(contentsOf: icon) else {
            throw UpdaterBrandingError.missingLogo
        }
        original.size = NSSize(width: 18, height: 18)
        guard let expectedLogo = original.tiffRepresentation,
              let template = NSImage(contentsOf: statusIcon) else {
            throw UpdaterBrandingError.missingLogo
        }
        template.size = NSSize(width: 18, height: 18)
        guard let expectedStatus = template.tiffRepresentation else {
            throw UpdaterBrandingError.missingLogo
        }
        let logo = try UpdaterBranding.logo(in: bundle)
        let statusLogo = try UpdaterBranding.statusLogo(in: bundle)
        let heading = UpdaterBranding.heading(logo: logo)
        let button = NSButton(frame: NSRect(x: 0, y: 0, width: 28, height: 28))
        button.isBordered = false
        button.title = "Stainless !"
        let preview = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: 168))
        preview.appearance = NSAppearance(named: .aqua)

        for (index, state) in [
            ("Ready", false), ("Checking/downloading preview...", false),
            ("Synthetic update failure", true), ("Recovered", false),
        ].enumerated() {
            UpdaterBranding.show(on: button, logo: statusLogo, message: state.0, isError: state.1)
            try expect(button.image?.tiffRepresentation == expectedStatus && button.image?.isTemplate == true
                && button.title.isEmpty && button.imagePosition == .imageOnly,
                "Only the one-color Stainless template must appear in normal, busy, error and recovery states")
            try expect(button.toolTip == "Stainless Updates\n\(state.0)"
                && button.accessibilityHelp() == state.0
                && button.accessibilityLabel() == (state.1 ? "Stainless Updates, update error" : "Stainless Updates"),
                "Status details and accessible error state must update without replacing the product identity")
            let rendered = NSButton(frame: NSRect(x: 12, y: 128 - index * 36, width: 28, height: 28))
            rendered.isBordered = false
            UpdaterBranding.show(on: rendered, logo: statusLogo, message: state.0, isError: state.1)
            preview.addSubview(rendered)
            let label = NSTextField(labelWithString: state.0)
            label.frame = NSRect(x: 52, y: 132 - index * 36, width: 236, height: 22)
            preview.addSubview(label)
        }
        try expect(heading.title == "Stainless Updates"
            && heading.image?.tiffRepresentation == expectedLogo && heading.image?.isTemplate == false
            && !heading.isEnabled,
            "The menu must have a branded, non-actionable heading")
        try snapshot(preview, to: URL(fileURLWithPath: CommandLine.arguments[3]))
        for (file, isStatus) in [(statusIcon, true), (icon, false)] {
            for invalid in [nil, Data("synthetic invalid image".utf8)] {
                try checkInvalidBranding(at: file, replacement: invalid, isStatus: isStatus,
                                         bundle: bundle, button: button)
            }
        }
        print("Stainless updater identity, one-color menu-bar template, visible/accessibility error state and offscreen branding checks passed.")
    }

    private static func checkInvalidBranding(
        at file: URL, replacement invalid: Data?, isStatus: Bool, bundle: Bundle, button: NSButton
    ) throws {
        if let invalid {
            try invalid.write(to: file)
        } else {
            try FileManager.default.removeItem(at: file)
        }
        do {
            _ = isStatus ? try UpdaterBranding.statusLogo(in: bundle) : try UpdaterBranding.logo(in: bundle)
            throw NSError(domain: "StainlessBrandingTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing/invalid branding must not use a generic product icon"])
        } catch UpdaterBrandingError.missingLogo {
            let message = UpdaterBrandingError.missingLogo.localizedDescription
            UpdaterBranding.show(on: button, logo: nil, message: message, isError: true)
            try expect(button.image == nil && button.title == "!"
                && button.toolTip == "Stainless Updates\n\(message)"
                && button.accessibilityLabel() == "Stainless Updates, update error"
                && button.accessibilityHelp() == message,
                "Missing branding must remain visibly actionable and accessible, not an empty status item")
        }
    }

    private static func snapshot(_ view: NSView, to destination: URL) throws {
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw NSError(domain: "StainlessBrandingTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Cannot render the offscreen branding fixture"])
        }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let image = bitmap.cgImage,
              let context = CGContext(data: nil, width: bitmap.pixelsWide, height: bitmap.pixelsHigh,
                bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw NSError(domain: "StainlessBrandingTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Cannot composite the offscreen branding fixture"])
        }
        context.setFillColor(CGColor(gray: 0.94, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: bitmap.pixelsWide, height: bitmap.pixelsHigh))
        context.draw(image, in: CGRect(x: 0, y: 0, width: bitmap.pixelsWide, height: bitmap.pixelsHigh))
        guard let composite = context.makeImage(),
              let png = NSBitmapImageRep(cgImage: composite).representation(using: .png, properties: [:]) else {
            throw NSError(domain: "StainlessBrandingTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Cannot encode the offscreen branding fixture"])
        }
        try png.write(to: destination)
    }
}
