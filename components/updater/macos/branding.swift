import AppKit

enum UpdaterBrandingError: LocalizedError {
    case missingLogo

    var errorDescription: String? {
        "Stainless Updates cannot load its bundled logo. Reinstall Stainless."
    }
}

@MainActor
enum UpdaterBranding {
    static func logo(in bundle: Bundle) throws -> NSImage {
        let image = try bundledImage("CFBundleIconFile", in: bundle)
        image.isTemplate = false
        image.accessibilityDescription = "Stainless logo"
        return image
    }

    // A one-color template, so the menu bar tints it for light, dark and highlighted states.
    static func statusLogo(in bundle: Bundle) throws -> NSImage {
        let image = try bundledImage("StainlessStatusIconFile", in: bundle)
        image.isTemplate = true
        image.accessibilityDescription = "Stainless"
        return image
    }

    private static func bundledImage(_ key: String, in bundle: Bundle) throws -> NSImage {
        guard let name = bundle.object(forInfoDictionaryKey: key) as? String,
              let url = bundle.url(forResource: name, withExtension: nil),
              let image = NSImage(contentsOf: url), image.isValid else {
            throw UpdaterBrandingError.missingLogo
        }
        image.size = NSSize(width: 18, height: 18)
        return image
    }

    static func heading(logo: NSImage?) -> NSMenuItem {
        let item = NSMenuItem(title: "Stainless Updates", action: nil, keyEquivalent: "")
        item.image = logo
        item.isEnabled = false
        return item
    }

    static func show(
        on button: NSButton, logo: NSImage?, message: String, isError: Bool = false
    ) {
        button.title = logo == nil ? "!" : ""
        button.image = logo
        button.imagePosition = logo == nil ? .noImage : .imageOnly
        button.imageScaling = .scaleProportionallyDown
        button.toolTip = "Stainless Updates\n\(message)"
        button.setAccessibilityLabel(isError ? "Stainless Updates, update error" : "Stainless Updates")
        button.setAccessibilityHelp(message)
    }
}
