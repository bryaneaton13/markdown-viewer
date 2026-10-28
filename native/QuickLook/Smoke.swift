import Foundation

@main
struct Smoke {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else { fatalError("usage: quicklook-smoke FILE.md OUT.html") }
        let data = try MarkdownPreview.render(URL(fileURLWithPath: CommandLine.arguments[1]))
        let html = String(decoding: data, as: UTF8.self)
        guard html.contains("<!doctype html>"), html.contains("Content-Security-Policy"), !html.contains("<script") else {
            fatalError("Invalid preview document")
        }
        try data.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
        print("Quick Look shared renderer: \(data.count) HTML bytes; wrote \(CommandLine.arguments[2])")
    }
}
