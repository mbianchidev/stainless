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
        try expect(CommandLine.arguments.count == 3, "Expected the logo and offscreen snapshot paths")
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
        try PropertyListSerialization.data(fromPropertyList: [
            "CFBundleIdentifier": "dev.stainless.synthetic-branding",
            "CFBundlePackageType": "APPL",
            "CFBundleIconFile": "stainless.png",
        ], format: .xml, options: 0).write(to: root.appendingPathComponent("Contents/Info.plist"))
        guard let bundle = Bundle(url: root) else {
            throw UpdaterBrandingError.missingLogo
        }
        guard let original = NSImage(contentsOf: icon) else {
            throw UpdaterBrandingError.missingLogo
        }
        original.size = NSSize(width: 18, height: 18)
        guard let expectedLogo = original.tiffRepresentation else {
            throw UpdaterBrandingError.missingLogo
        }
        let logo = try UpdaterBranding.logo(in: bundle)
        let heading = UpdaterBranding.heading(logo: logo)
        let button = NSButton(frame: NSRect(x: 0, y: 0, width: 180, height: 28))
        button.isBordered = false
        let preview = NSView(frame: NSRect(x: 0, y: 0, width: 360, height: 132))
        preview.appearance = NSAppearance(named: .aqua)

        for (index, state) in [
            ("Ready", false), ("Synthetic update failure", true), ("Recovered", false),
        ].enumerated() {
            UpdaterBranding.show(on: button, logo: logo, message: state.0, isError: state.1)
            try expect(button.image?.tiffRepresentation == expectedLogo && button.image?.isTemplate == false
                && button.title == (state.1 ? "Stainless !" : "Stainless"),
                "The Stainless name and original logo must survive normal, error and recovery states")
            try expect(button.toolTip == "Stainless Updates\n\(state.0)"
                && button.accessibilityHelp() == state.0
                && button.accessibilityLabel() == (state.1 ? "Stainless Updates, update error" : "Stainless Updates"),
                "Status details and accessible error state must update without replacing the product identity")
            let rendered = NSButton(frame: NSRect(x: 12, y: 92 - index * 36, width: 180, height: 28))
            rendered.isBordered = false
            UpdaterBranding.show(on: rendered, logo: logo, message: state.0, isError: state.1)
            preview.addSubview(rendered)
            let label = NSTextField(labelWithString: state.0)
            label.frame = NSRect(x: 196, y: 96 - index * 36, width: 156, height: 22)
            preview.addSubview(label)
        }
        try expect(heading.title == "Stainless Updates"
            && heading.image?.tiffRepresentation == expectedLogo && !heading.isEnabled,
            "The menu must have a branded, non-actionable heading")
        try snapshot(preview, to: URL(fileURLWithPath: CommandLine.arguments[2]))
        for invalid in [nil, Data("synthetic invalid image".utf8)] {
            if let invalid {
                try invalid.write(to: icon)
            } else {
                try FileManager.default.removeItem(at: icon)
            }
            do {
                _ = try UpdaterBranding.logo(in: bundle)
                throw NSError(domain: "StainlessBrandingTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Missing/invalid branding must not use a generic product icon"])
            } catch UpdaterBrandingError.missingLogo {}
        }
        print("Stainless updater identity, visible/accessibility error state and offscreen branding checks passed.")
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
