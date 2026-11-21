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
        let background = try stainlessDMGBackground()
        precondition(background.width == 1440 && background.height == 960, "Installer artwork must retain Retina dimensions")
        let backgroundData = background.dataProvider!.data! as Data
        func arrowColumn(_ x: Int) -> Int {
            (320..<640).filter { y in
                let offset = y * background.bytesPerRow + x * 4
                return backgroundData[offset] < 8 && backgroundData[offset + 1] < 8 &&
                    backgroundData[offset + 2] < 8 && backgroundData[offset + 3] == 255
            }.count
        }
        precondition(arrowColumn(660) > 0 && arrowColumn(790) > arrowColumn(660), "Installer arrow must point right")
        precondition(arrowColumn(580) == 0 && arrowColumn(850) == 0, "Installer arrow must leave space for the app and Applications")
        try checkStatusTemplate()
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
            let logo = try loadImage(CommandLine.arguments[1])
            let template = try stainlessStatusTemplate(logo: logo, pixels: 1024)
            let mask = template.dataProvider!.data! as Data
            var solid = 0, kept = 0
            for offset in stride(from: 0, to: source.count, by: 4) {
                precondition(mask[offset] == 0 && mask[offset + 1] == 0 && mask[offset + 2] == 0,
                    "The menu-bar template must be one color")
                if source[offset + 3] > 127 {
                    solid += 1
                    kept += mask[offset + 3] > 127 ? 1 : 0
                }
                precondition(mask[offset + 3] <= source[offset + 3], "The template must stay inside the S")
            }
            precondition(kept < solid && kept * 100 > solid * 85,
                "The template must keep the S silhouette and cut only its interior folds")
            let small = try stainlessStatusTemplate(logo: logo, pixels: 36)
            precondition(small.width == 36 && (small.dataProvider!.data! as Data).enumerated()
                .contains { $0.offset % 4 == 3 && $0.element > 200 }, "The 18-point Retina template must be visible")
            print("Verified the one-color menu-bar template keeps \(kept) of \(solid) S pixels")
        }
        print("Black icon background, transparent corners, unchanged logo colors and Retina installer arrow verified")
    }

    static func checkStatusTemplate() throws {
        let fixture = CGContext(
            data: nil, width: 1024, height: 1024, bitsPerComponent: 8,
            bytesPerRow: 4096, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!
        fixture.setFillColor(CGColor(red: 0.2, green: 0.2, blue: 0.22, alpha: 1))
        fixture.fill(CGRect(x: 200, y: 200, width: 624, height: 624))
        fixture.setFillColor(CGColor(red: 0.8, green: 0.8, blue: 0.82, alpha: 1))
        fixture.fill(CGRect(x: 208, y: 208, width: 608, height: 608))
        fixture.setFillColor(CGColor(red: 0.2, green: 0.2, blue: 0.22, alpha: 1))
        fixture.fill(CGRect(x: 300, y: 508, width: 424, height: 8))
        let template = try stainlessStatusTemplate(logo: fixture.makeImage()!, pixels: 1024)
        let data = template.dataProvider!.data! as Data
        func alpha(_ x: Int, _ y: Int) -> UInt8 {
            data[y * template.bytesPerRow + x * 4 + 3]
        }
        for offset in stride(from: 0, to: data.count, by: 4) {
            precondition(data[offset] == 0 && data[offset + 1] == 0 && data[offset + 2] == 0,
                "The menu-bar template must be one color")
        }
        precondition(alpha(100, 100) == 0, "The template background must stay transparent")
        precondition(alpha(400, 300) == 255 && alpha(512, 560) == 255, "The filled shape must stay opaque")
        precondition(alpha(203, 512) == 255 && alpha(512, 203) == 255, "The outer outline must stay in the silhouette")
        precondition(alpha(512, 511) == 0 && alpha(512, 522) == 0, "Interior folds must be cut and widened")
        do {
            _ = try stainlessStatusTemplate(logo: fixture.makeImage()!, pixels: 0)
            preconditionFailure("An empty template size must be rejected")
        } catch IconError.cannotRender {
        }
    }

    static func loadImage(_ path: String) throws -> CGImage {
        guard let reader = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let image = CGImageSourceCreateImageAtIndex(reader, 0, nil) else {
            throw IconError.invalidImage
        }
        return image
    }

    static func rgba(_ path: String) throws -> Data {
        let image = try loadImage(path)
        guard let context = CGContext(
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
