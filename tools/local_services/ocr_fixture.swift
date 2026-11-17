import AppKit
import CryptoKit
import Foundation

private struct Corpus: Decodable {
    struct Generator: Decodable {
        let width: Int
        let height: Int
        let font: String
        let fontSize: Double
        let textX: Double
        let textY: Double
        let foreground: String
        let background: String
        let format: String
    }

    struct Limits: Decodable {
        let maximumAxisPixels: Int
        let maximumImagePixels: Int
    }

    struct Case: Decodable {
        let language: String
        let text: String
    }

    let generator: Generator
    let limits: Limits
    let cases: [Case]
}

private struct FixtureFailure: Error, CustomStringConvertible {
    let description: String
}

private func generate(_ corpus: Corpus, into directory: URL) throws -> Data {
    let config = corpus.generator
    let product = config.width.multipliedReportingOverflow(by: config.height)
    guard config.width > 0, config.height > 0,
          config.width <= corpus.limits.maximumAxisPixels,
          config.height <= corpus.limits.maximumAxisPixels,
          !product.overflow, product.partialValue <= corpus.limits.maximumImagePixels,
          config.foreground == "black", config.background == "white",
          config.format == "PGM",
          let font = NSFont(name: config.font, size: config.fontSize)
    else {
        throw FixtureFailure(description: "Invalid raster geometry, colors, format or missing font")
    }
    let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
    guard values.isDirectory == true, values.isSymbolicLink == false else {
        throw FixtureFailure(description: "Use an existing owned unlinked output directory")
    }
    var records: [[String: Any]] = []
    for (index, item) in corpus.cases.enumerated() {
        var pixels = [UInt8](repeating: 255, count: product.partialValue)
        try pixels.withUnsafeMutableBytes { bytes in
            guard let context = CGContext(
                data: bytes.baseAddress, width: config.width, height: config.height,
                bitsPerComponent: 8, bytesPerRow: config.width,
                space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue
            ) else {
                throw FixtureFailure(description: "Cannot create the offscreen raster context")
            }
            context.setFillColor(gray: 1, alpha: 1)
            context.fill(CGRect(x: 0, y: 0, width: config.width, height: config.height))
            context.translateBy(x: 0, y: CGFloat(config.height))
            context.scaleBy(x: 1, y: -1)
            NSGraphicsContext.saveGraphicsState()
            defer { NSGraphicsContext.restoreGraphicsState() }
            NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: NSColor.black,
            ]
            (item.text as NSString).draw(
                at: NSPoint(x: config.textX, y: config.textY),
                withAttributes: attributes
            )
        }
        let name = "case-\(index).pgm"
        let destination = directory.appendingPathComponent(name)
        guard !FileManager.default.fileExists(atPath: destination.path) else {
            throw FixtureFailure(description: "Preserve an existing fixture: \(name)")
        }
        var data = Data("P5\n\(config.width) \(config.height)\n255\n".utf8)
        data.append(contentsOf: pixels)
        try data.write(to: destination, options: .atomic)
        records.append([
            "file": name,
            "language": item.language,
            "bytes": data.count,
            "sha256": SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined(),
        ])
    }
    return try JSONSerialization.data(withJSONObject: [
        "font_name": font.fontName,
        "width": config.width,
        "height": config.height,
        "cases": records,
    ], options: [.sortedKeys, .prettyPrinted])
}

do {
    guard CommandLine.arguments.count == 3 else {
        throw FixtureFailure(description: "Usage: ocr-fixture CORPUS_JSON OWNED_OUTPUT_DIRECTORY")
    }
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    let corpus = try decoder.decode(
        Corpus.self,
        from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    )
    let result = try generate(
        corpus, into: URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
    )
    FileHandle.standardOutput.write(result)
    FileHandle.standardOutput.write(Data("\n".utf8))
} catch {
    FileHandle.standardError.write(Data("OCR fixture generation failed: \(error)\n".utf8))
    exit(1)
}
