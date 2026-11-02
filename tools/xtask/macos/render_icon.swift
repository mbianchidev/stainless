import Foundation

@main
struct RenderIcon {
    static func main() {
        guard (4...5).contains(CommandLine.arguments.count),
              let pixels = Int(CommandLine.arguments[3]), (1...1024).contains(pixels) else {
            FileHandle.standardError.write(Data("Usage: render-icon SOURCE.png OUTPUT.png PIXELS (1...1024) [wordmark-light|wordmark-dark]\n".utf8))
            exit(2)
        }
        do {
            let source = URL(fileURLWithPath: CommandLine.arguments[1])
            let destination = URL(fileURLWithPath: CommandLine.arguments[2])
            if CommandLine.arguments.count == 5 {
                let mode = CommandLine.arguments[4]
                guard mode == "wordmark-light" || mode == "wordmark-dark" else {
                    FileHandle.standardError.write(Data("Unsupported icon mode \(mode): expected wordmark-light or wordmark-dark\n".utf8))
                    exit(2)
                }
                try writeStainlessWordmark(source: source, destination: destination,
                                          pixels: pixels, dark: mode == "wordmark-dark")
            } else {
                try writeStainlessIcon(source: source, destination: destination, pixels: pixels)
            }
        } catch {
            FileHandle.standardError.write(Data("Cannot render Stainless icon from \(CommandLine.arguments[1]) to \(CommandLine.arguments[2]): \(error)\n".utf8))
            exit(1)
        }
    }
}
