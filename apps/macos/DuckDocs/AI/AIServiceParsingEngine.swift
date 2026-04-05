//
//  AIServiceParsingEngine.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-04-05.
//

import Foundation
import AppKit

/// Adapter that lets the current AIService satisfy the schema-first parsing seam.
@MainActor
final class AIServiceParsingEngine: DocumentParsingEngine {
    private let aiService: AIService
    private let converter = DocumentConverter()

    init(aiService: AIService) {
        self.aiService = aiService
    }

    var engineID: String {
        "\(aiService.providerType.rawValue.lowercased())/\(aiService.modelId)"
    }

    func parseDocument(
        request: SchemaParseRequest,
        onEvent: @escaping @Sendable (SchemaProcessEvent) -> Void
    ) async throws -> SchemaParseResult {
        let fileURL = URL(fileURLWithPath: request.input.path)
        let job = try DocumentImportJob(fileURL: fileURL)
        onEvent(.queued)
        onEvent(.documentOpened(format: request.input.format))

        let images = try await converter.convert(job)
        let total = images.count
        var pages: [SchemaParsedPage] = []
        var assets: [SchemaOutputAsset] = []
        var successCount = 0
        var failedCount = 0

        for (index, image) in images.enumerated() {
            onEvent(.convertingPages(current: index + 1, total: total))
            if let asset = imageAsset(for: image, index: index) {
                assets.append(asset)
            }
            onEvent(.parsing(current: index + 1, total: total))

            do {
                let markdown = try await aiService.analyzeImage(image, prompt: prompt(for: request))
                successCount += 1
                pages.append(SchemaParsedPage(
                    index: index,
                    markdown: markdown,
                    plainText: markdown,
                    svg: nil,
                    imageAssetPath: assets.last?.relativePath,
                    errorMessage: nil
                ))
            } catch {
                failedCount += 1
                pages.append(SchemaParsedPage(
                    index: index,
                    markdown: nil,
                    plainText: nil,
                    svg: nil,
                    imageAssetPath: assets.last?.relativePath,
                    errorMessage: error.localizedDescription
                ))
            }
        }

        onEvent(.packaging)
        onEvent(.completed)

        let metadata = SchemaParseMetadata(
            engineID: engineID,
            durationMilliseconds: 0,
            pageCount: pages.count
        )
        let markdown = MarkdownGenerator().generate(
            title: request.output?.name ?? "DuckDocs Import",
            sections: pages.enumerated().map { index, page in
                MarkdownGenerator.Section(
                    title: "Page \(index + 1)",
                    detail: "**Source:** \(request.input.format.displayName)",
                    imagePath: page.imageAssetPath ?? "images/page_\(index + 1).png",
                    body: page.markdown ?? page.plainText ?? "_AI analysis unavailable for page \(index + 1)._"
                )
            }
        )
        return SchemaParseResult(
            version: request.version,
            markdown: markdown,
            pages: pages,
            assets: assets,
            metadata: metadata,
            successCount: successCount,
            failedCount: failedCount
        )
    }

    private func prompt(for request: SchemaParseRequest) -> String {
        PromptTemplate(rawValue: request.template)?.prompt ?? aiService.selectedTemplate.prompt
    }

    private func imageAsset(for image: NSImage, index: Int) -> SchemaOutputAsset? {
        guard let tiff = image.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiff),
              let pngData = bitmap.representation(using: .png, properties: [:]) else {
            return nil
        }
        return SchemaOutputAsset(
            relativePath: "images/page_\(index + 1).png",
            mimeType: "image/png",
            base64: pngData.base64EncodedString()
        )
    }
}
