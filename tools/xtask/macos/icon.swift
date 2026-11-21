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
    let image = try stainlessIcon(logo: readLogo(source), pixels: pixels)
    try writePNG(image: image, destination: destination)
}

// A one-color menu-bar silhouette of the S. macOS tints template images from their
// alpha channel only. The dark outline strokes inside the S (its ribbon folds) are cut
// out and widened so the folds stay visible at menu-bar size; the outer outline stays.
func stainlessStatusTemplate(logo: CGImage, pixels: Int) throws -> CGImage {
    let size = 1024
    guard pixels > 0, pixels <= size else {
        throw IconError.cannotRender
    }
    var source = [UInt8](repeating: 0, count: size * size * 4)
    let drawn = source.withUnsafeMutableBytes { buffer -> Bool in
        guard let context = CGContext(
            data: buffer.baseAddress, width: size, height: size, bitsPerComponent: 8,
            bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            return false
        }
        context.interpolationQuality = .high
        context.draw(logo, in: CGRect(x: 0, y: 0, width: size, height: size))
        return true
    }
    guard drawn else {
        throw IconError.cannotRender
    }
    var solid = [Bool](repeating: false, count: size * size)
    var stroke = [Bool](repeating: false, count: size * size)
    for index in 0..<(size * size) {
        let alpha = Double(source[index * 4 + 3])
        guard alpha > 127 else { continue }
        solid[index] = true
        let luminance = (0.2126 * Double(source[index * 4]) + 0.7152 * Double(source[index * 4 + 1])
            + 0.0722 * Double(source[index * 4 + 2])) / alpha
        stroke[index] = luminance < 0.33
    }
    let interior = morphology(solid, size: size, radius: 16, erode: true)
    let folds = morphology(zip(stroke, interior).map { $0 && $1 }, size: size, radius: 12, erode: false)
    var mask = [UInt8](repeating: 0, count: size * size * 4)
    for index in 0..<(size * size) where !folds[index] {
        mask[index * 4 + 3] = source[index * 4 + 3]
    }
    let image = mask.withUnsafeMutableBytes { buffer -> CGImage? in
        CGContext(
            data: buffer.baseAddress, width: size, height: size, bitsPerComponent: 8,
            bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )?.makeImage()
    }
    guard let image, let context = CGContext(
        data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
        bytesPerRow: pixels * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw IconError.cannotRender
    }
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    guard let template = context.makeImage() else {
        throw IconError.cannotRender
    }
    return template
}

// Square erosion/dilation, applied separably with sliding-window counts.
private func morphology(_ mask: [Bool], size: Int, radius: Int, erode: Bool) -> [Bool] {
    func pass(_ input: [Bool], horizontal: Bool) -> [Bool] {
        var output = [Bool](repeating: false, count: size * size)
        for line in 0..<size {
            let at = { (position: Int) in horizontal ? line * size + position : position * size + line }
            var count = 0
            for position in 0..<min(radius, size) where input[at(position)] {
                count += 1
            }
            for position in 0..<size {
                if position + radius < size, input[at(position + radius)] {
                    count += 1
                }
                if position - radius - 1 >= 0, input[at(position - radius - 1)] {
                    count -= 1
                }
                output[at(position)] = erode ? count == 2 * radius + 1 : count > 0
            }
        }
        return output
    }
    return pass(pass(mask, horizontal: true), horizontal: false)
}

func writeStainlessStatusTemplate(source: URL, destination: URL, pixels: Int) throws {
    let image = try stainlessStatusTemplate(logo: readLogo(source), pixels: pixels)
    try writePNG(image: image, destination: destination, properties: [
        kCGImagePropertyDPIWidth: 144,
        kCGImagePropertyDPIHeight: 144
    ] as CFDictionary)
}

private func readLogo(_ source: URL) throws -> CGImage {
    guard let reader = CGImageSourceCreateWithURL(source as CFURL, nil),
          let logo = CGImageSourceCreateImageAtIndex(reader, 0, nil) else {
        throw IconError.invalidImage
    }
    return logo
}

func writeStainlessWordmark(source: URL, destination: URL, pixels: Int, dark: Bool) throws {
    let logo = try readLogo(source)
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

func stainlessDMGBackground() throws -> CGImage {
    guard let context = CGContext(
        data: nil, width: 1440, height: 960, bitsPerComponent: 8,
        bytesPerRow: 1440 * 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw IconError.cannotRender
    }
    context.scaleBy(x: 2, y: 2)
    context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: 720, height: 480))
    context.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
    context.fill(CGRect(x: 316, y: 275, width: 68, height: 10))
    context.move(to: CGPoint(x: 380, y: 259))
    context.addLine(to: CGPoint(x: 410, y: 280))
    context.addLine(to: CGPoint(x: 380, y: 301))
    context.closePath()
    context.fillPath()
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = .center
    for (text, size, weight, y) in [
        ("Stainless", CGFloat(32), NSFont.Weight.semibold, CGFloat(388)),
        ("Drag Stainless to Applications", CGFloat(18), NSFont.Weight.regular, CGFloat(76))
    ] {
        NSAttributedString(string: text, attributes: [
            .font: NSFont.systemFont(ofSize: size, weight: weight),
            .foregroundColor: NSColor.black,
            .paragraphStyle: paragraph
        ]).draw(in: CGRect(x: 40, y: y, width: 640, height: 48))
    }
    NSGraphicsContext.restoreGraphicsState()
    guard let image = context.makeImage() else {
        throw IconError.cannotRender
    }
    return image
}

func writeStainlessDMGBackground(destination: URL) throws {
    try writePNG(image: stainlessDMGBackground(), destination: destination, properties: [
        kCGImagePropertyDPIWidth: 144,
        kCGImagePropertyDPIHeight: 144
    ] as CFDictionary)
}

private func writePNG(image: CGImage, destination: URL, properties: CFDictionary? = nil) throws {
    guard let writer = CGImageDestinationCreateWithURL(
        destination as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else {
        throw IconError.cannotWrite
    }
    CGImageDestinationAddImage(writer, image, properties)
    guard CGImageDestinationFinalize(writer) else {
        throw IconError.cannotWrite
    }
}
