import CoreGraphics
import Foundation
import ImageIO

@main
struct IconTests {
    static func main() throws {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let fixture = CGContext(
            data: nil, width: 1024, height: 1024, bitsPerComponent: 8,
            bytesPerRow: 4096, space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!
        fixture.setFillColor(CGColor(red: 0.6, green: 0.6, blue: 0.6, alpha: 1))
        fixture.fill(CGRect(x: 450, y: 450, width: 124, height: 124))
        let logo = fixture.makeImage()!
        let original = logo.dataProvider!.data! as Data
        let image = try stainlessIcon(logo: logo, pixels: 1024)
        let data = image.dataProvider!.data! as Data
        func pixel(_ x: Int, _ y: Int) -> [UInt8] {
            Array(data[(y * image.bytesPerRow + x * 4)..<(y * image.bytesPerRow + x * 4 + 4)])
        }
        precondition(pixel(100, 512) == [0, 0, 0, 255], "Icon background must be opaque black")
        precondition(pixel(0, 0) == [0, 0, 0, 0], "Rounded icon corners must remain transparent")
        let center = 512 * logo.bytesPerRow + 512 * 4
        precondition(pixel(512, 512) == Array(original[center..<(center + 4)]), "The S colors must be preserved")
        if CommandLine.arguments.count == 3 {
            let source = try rgba(CommandLine.arguments[1])
            let icon = try rgba(CommandLine.arguments[2])
            precondition(source.count == icon.count, "Master icon dimensions changed")
            var visible = 0
            for offset in stride(from: 0, to: source.count, by: 4) where source[offset + 3] > 0 {
                visible += 1
                precondition(
                    source[offset] == icon[offset] &&
                    source[offset + 1] == icon[offset + 1] &&
                    source[offset + 2] == icon[offset + 2] &&
                    icon[offset + 3] >= source[offset + 3],
                    "Original S color changed at pixel \(offset / 4)"
                )
            }
            precondition(visible > 0, "The master logo contains no visible S pixels")
            print("Verified all \(visible) visible original S pixels without recoloring")
        }
        print("Black icon background, transparent corners and unchanged logo colors verified")
    }

    static func rgba(_ path: String) throws -> Data {
        guard let reader = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let image = CGImageSourceCreateImageAtIndex(reader, 0, nil),
              let context = CGContext(
                data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                bytesPerRow: image.width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
              ) else {
            throw IconError.invalidImage
        }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return context.makeImage()!.dataProvider!.data! as Data
    }
}
