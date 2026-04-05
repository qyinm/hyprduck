//
//  EngineContract.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-04-05.
//

import Foundation
import AppKit

struct SchemaParseOptions: Codable, Equatable {
    var preserveImages: Bool = true
    var emitStructuredJSON: Bool = false
    var emitSVG: Bool = false
    var languageHints: [String] = []
    var debugRequestPath: String?
    var debugResultPath: String?

    enum CodingKeys: String, CodingKey {
        case preserveImages = "preserve_images"
        case emitStructuredJSON = "emit_structured_json"
        case emitSVG = "emit_svg"
        case languageHints = "language_hints"
        case debugRequestPath = "debug_request_path"
        case debugResultPath = "debug_result_path"
    }
}

struct SchemaParseInput: Codable, Equatable {
    let path: String
    let format: DocumentFormat
}

struct SchemaParseOutputTarget: Codable, Equatable {
    let rootDirectory: String?
    let name: String?

    enum CodingKeys: String, CodingKey {
        case rootDirectory = "root_dir"
        case name
    }
}

struct SchemaParseRequest: Codable, Equatable {
    let version: String
    let input: SchemaParseInput
    let template: String
    let options: SchemaParseOptions
    let output: SchemaParseOutputTarget?

    init(job: DocumentImportJob, template: PromptTemplate, options: SchemaParseOptions = SchemaParseOptions()) {
        self.version = "1"
        self.input = SchemaParseInput(path: job.fileURL.path, format: job.format)
        self.template = template.rawValue
        self.options = options
        self.output = SchemaParseOutputTarget(rootDirectory: nil, name: job.outputName)
    }
}

struct SchemaParsedPage: Codable, Equatable {
    let index: Int
    let markdown: String?
    let plainText: String?
    let svg: String?
    let imageAssetPath: String?
    let errorMessage: String?

    enum CodingKeys: String, CodingKey {
        case index
        case markdown
        case plainText = "plain_text"
        case svg
        case imageAssetPath = "image_asset_path"
        case errorMessage = "error_message"
    }
}

struct SchemaOutputAsset: Codable, Equatable {
    let relativePath: String
    let mimeType: String
    let base64: String

    enum CodingKeys: String, CodingKey {
        case relativePath = "relative_path"
        case mimeType = "mime_type"
        case base64
    }
}

struct SchemaParseMetadata: Codable, Equatable {
    let engineID: String
    let durationMilliseconds: Int
    let pageCount: Int

    enum CodingKeys: String, CodingKey {
        case engineID = "engine_id"
        case durationMilliseconds = "duration_ms"
        case pageCount = "page_count"
    }
}

struct SchemaParseResult: Codable, Equatable {
    let version: String
    let markdown: String
    let pages: [SchemaParsedPage]
    let assets: [SchemaOutputAsset]
    let metadata: SchemaParseMetadata
    let successCount: Int
    let failedCount: Int

    enum CodingKeys: String, CodingKey {
        case version
        case markdown
        case pages
        case assets
        case metadata
        case successCount = "success_count"
        case failedCount = "failed_count"
    }
}

enum SchemaProcessEvent: Equatable {
    case queued
    case documentOpened(format: DocumentFormat)
    case convertingPages(current: Int, total: Int)
    case parsing(current: Int, total: Int)
    case packaging
    case completed
    case failed(message: String)
}

extension SchemaProcessEvent: Decodable {
    private enum CodingKeys: String, CodingKey {
        case type
        case format
        case current
        case total
        case message
    }

    private enum EventType: String, Decodable {
        case queued
        case documentOpened = "document_opened"
        case convertingPages = "converting_pages"
        case parsing
        case packaging
        case completed
        case failed
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(EventType.self, forKey: .type)
        switch type {
        case .queued:
            self = .queued
        case .documentOpened:
            self = .documentOpened(format: try container.decode(DocumentFormat.self, forKey: .format))
        case .convertingPages:
            self = .convertingPages(
                current: try container.decode(Int.self, forKey: .current),
                total: try container.decode(Int.self, forKey: .total)
            )
        case .parsing:
            self = .parsing(
                current: try container.decode(Int.self, forKey: .current),
                total: try container.decode(Int.self, forKey: .total)
            )
        case .packaging:
            self = .packaging
        case .completed:
            self = .completed
        case .failed:
            self = .failed(message: try container.decode(String.self, forKey: .message))
        }
    }
}

protocol DocumentParsingEngine: AnyObject {
    var engineID: String { get }
    func parseDocument(
        request: SchemaParseRequest,
        onEvent: @escaping @Sendable (SchemaProcessEvent) -> Void
    ) async throws -> SchemaParseResult
    func cancelCurrentRun()
}

extension DocumentParsingEngine {
    func cancelCurrentRun() {}
}
