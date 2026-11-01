import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

enum IconError: Error {
    case invalidImage
    case cannotRender
    case cannotWrite
}

func stainlessIcon(logo: CGImage, pixels: Int) throws -> CGImage {
    guard pixels > 0, let context = CGContext(
        data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
        bytesPerRow: pixels * 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw IconError.cannotRender
    }
    let scale = CGFloat(pixels) / 1024
    let tile = CGRect(x: 64 * scale, y: 64 * scale, width: 896 * scale, height: 896 * scale)
    context.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
    context.addPath(CGPath(roundedRect: tile, cornerWidth: 180 * scale, cornerHeight: 180 * scale, transform: nil))
    context.fillPath()
    context.interpolationQuality = .high
    context.draw(logo, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    guard let image = context.makeImage() else {
        throw IconError.cannotRender
    }
    return image
}

func writeStainlessIcon(source: URL, destination: URL, pixels: Int) throws {
    guard let reader = CGImageSourceCreateWithURL(source as CFURL, nil),
          let logo = CGImageSourceCreateImageAtIndex(reader, 0, nil) else {
        throw IconError.invalidImage
    }
    let image = try stainlessIcon(logo: logo, pixels: pixels)
    try writePNG(image: image, destination: destination)
}

func writeStainlessWordmark(source: URL, destination: URL, pixels: Int, dark: Bool) throws {
    guard let reader = CGImageSourceCreateWithURL(source as CFURL, nil),
          let logo = CGImageSourceCreateImageAtIndex(reader, 0, nil) else {
        throw IconError.invalidImage
    }
    let scale = CGFloat(pixels) / 22
    guard let context = CGContext(
        data: nil, width: Int(97 * scale), height: pixels, bitsPerComponent: 8,
        bytesPerRow: Int(97 * scale) * 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw IconError.cannotRender
    }
    context.draw(try stainlessIcon(logo: logo, pixels: pixels),
                 in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
    NSAttributedString(string: "Stainless", attributes: [
        .font: NSFont.systemFont(ofSize: 13 * scale, weight: .semibold),
        .foregroundColor: dark ? NSColor.white : NSColor.black
    ]).draw(in: CGRect(x: 25 * scale, y: 3 * scale, width: 72 * scale, height: 18 * scale))
    NSGraphicsContext.restoreGraphicsState()
    guard let image = context.makeImage() else {
        throw IconError.cannotRender
    }
    try writePNG(image: image, destination: destination)
}

private func writePNG(image: CGImage, destination: URL) throws {
    guard let writer = CGImageDestinationCreateWithURL(
        destination as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else {
        throw IconError.cannotWrite
    }
    CGImageDestinationAddImage(writer, image, nil)
    guard CGImageDestinationFinalize(writer) else {
        throw IconError.cannotWrite
    }
}
