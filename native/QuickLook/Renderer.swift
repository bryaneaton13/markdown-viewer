import Foundation

@_silgen_name("markdown_preview_html")
private func markdownPreviewHTML(_ bytes: UnsafePointer<UInt8>, _ count: Int) -> UnsafeMutablePointer<CChar>?
@_silgen_name("markdown_preview_free")
private func markdownPreviewFree(_ pointer: UnsafeMutablePointer<CChar>)

enum MarkdownPreview {
    static func render(_ url: URL) throws -> Data {
        guard ["md", "markdown"].contains(url.pathExtension.lowercased()) else {
            throw NSError(domain: "MarkdownPreview", code: 1, userInfo: [NSLocalizedDescriptionKey: "Only Markdown files are supported."])
        }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: 8 * 1024 * 1024 + 1) ?? Data()
        guard data.count <= 8 * 1024 * 1024 else {
            throw NSError(domain: "MarkdownPreview", code: 2, userInfo: [NSLocalizedDescriptionKey: "This preview supports files up to 8 MiB."])
        }
        // Non-null buffer even for an empty file.
        let bytes = Array(data) + [0]
        return try bytes.withUnsafeBufferPointer { buffer in
            guard let html = markdownPreviewHTML(buffer.baseAddress!, data.count) else {
                throw NSError(domain: "MarkdownPreview", code: 3, userInfo: [NSLocalizedDescriptionKey: "Could not render UTF-8 Markdown."])
            }
            defer { markdownPreviewFree(html) }
            return Data(String(cString: html).utf8)
        }
    }
}
