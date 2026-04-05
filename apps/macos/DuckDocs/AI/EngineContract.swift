//
//  EngineContract.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-04-05.
//

import Foundation
import AppKit

/// Schema-shaped parse options for the future engine boundary.
struct SchemaParseOptions: Codable, Equatable {
    var preserveImages: Bool = true
    var emitStructuredJSON: Bool = false
    var emitSVG: Bool = false
    var languageHints: [String] = []
}

struct SchemaParseInput: Codable, Equatable {
    let path: String
    let format: DocumentFormat
}

struct SchemaParseOutputTarget: Codable, Equatable {
    let rootDirectory: String?
    let name: String

    enum CodingKeys: String, CodingKey {
        case rootDirectory = "root_dir"
        case name
    }
}

/// Minimal local mirror of the repo-level schema contract.
struct SchemaParseRequest: Codable, Equatable {
    let version: String
    let input: SchemaParseInput
    let template: String
    let options: SchemaParseOptions
    let output: SchemaParseOutputTarget

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

    enum CodingKeys: String, CodingKey {
        case index
        case markdown
        case plainText = "plain_text"
        case svg
    }
}

struct SchemaOutputAsset {
    let relativePath: String
    let mimeType: String
    let data: Data
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

struct SchemaParseResult {
    let version: String
    let markdown: String
    let pages: [SchemaParsedPage]
    let assets: [SchemaOutputAsset]
    let metadata: SchemaParseMetadata

    init(markdown: String, pages: [SchemaParsedPage], assets: [SchemaOutputAsset], metadata: SchemaParseMetadata) {
        self.version = "1"
        self.markdown = markdown
        self.pages = pages
        self.assets = assets
        self.metadata = metadata
    }
}

/// Narrow engine seam for the import-first macOS app.
protocol DocumentParsingEngine {
    var engineID: String { get }

    func parsePage(
        image: NSImage,
        pageIndex: Int,
        request: SchemaParseRequest
    ) async throws -> SchemaParsedPage
}
