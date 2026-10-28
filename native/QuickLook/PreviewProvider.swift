import Cocoa
import QuickLookUI
import UniformTypeIdentifiers

@objc(PreviewProvider)
final class PreviewProvider: QLPreviewProvider, QLPreviewingController {
    func providePreview(for request: QLFilePreviewRequest, completionHandler handler: @escaping (QLPreviewReply?, Error?) -> Void) {
        let url = request.fileURL
        let reply = QLPreviewReply(dataOfContentType: .html, contentSize: CGSize(width: 900, height: 760)) { reply in
            reply.stringEncoding = .utf8
            reply.title = url.lastPathComponent
            return try MarkdownPreview.render(url)
        }
        handler(reply, nil)
    }
}
