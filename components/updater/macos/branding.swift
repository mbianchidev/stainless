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
        guard let name = bundle.object(forInfoDictionaryKey: "CFBundleIconFile") as? String,
              let url = bundle.url(forResource: name, withExtension: nil),
              let image = NSImage(contentsOf: url), image.isValid else {
            throw UpdaterBrandingError.missingLogo
        }
        image.size = NSSize(width: 18, height: 18)
        image.isTemplate = false
        image.accessibilityDescription = "Stainless logo"
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
        button.title = isError ? "Stainless !" : "Stainless"
        button.image = logo
        button.imagePosition = .imageLeading
        button.imageScaling = .scaleProportionallyDown
        button.toolTip = "Stainless Updates\n\(message)"
        button.setAccessibilityLabel(isError ? "Stainless Updates, update error" : "Stainless Updates")
        button.setAccessibilityHelp(message)
    }
}
