import Foundation

@main
struct RenderDMGBackground {
    static func main() {
        guard CommandLine.arguments.count == 2 else {
            FileHandle.standardError.write(Data("Usage: render-dmg-background OUTPUT.png\n".utf8))
            exit(2)
        }
        do {
            try writeStainlessDMGBackground(
                destination: URL(fileURLWithPath: CommandLine.arguments[1])
            )
        } catch {
            FileHandle.standardError.write(Data("Cannot render Stainless installer background: \(error)\n".utf8))
            exit(1)
        }
    }
}
