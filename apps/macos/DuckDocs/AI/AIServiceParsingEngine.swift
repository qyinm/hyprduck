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

    init(aiService: AIService) {
        self.aiService = aiService
    }

    var engineID: String {
        "\(aiService.providerType.rawValue.lowercased())/\(aiService.modelId)"
    }

    func parsePage(
        image: NSImage,
        pageIndex: Int,
        request: SchemaParseRequest
    ) async throws -> SchemaParsedPage {
        let markdown = try await aiService.analyzeImage(image, prompt: prompt(for: request))

        return SchemaParsedPage(
            index: pageIndex,
            markdown: markdown,
            plainText: markdown,
            svg: nil
        )
    }

    private func prompt(for request: SchemaParseRequest) -> String {
        PromptTemplate(rawValue: request.template)?.prompt ?? aiService.selectedTemplate.prompt
    }
}
