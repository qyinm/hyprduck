//
//  DocumentImportJob.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-03.
//

import Foundation

/// Document format types
enum DocumentFormat: String, CaseIterable {
    case pdf
    case docx
    case doc

    var displayName: String {
        switch self {
        case .pdf: return "PDF"
        case .docx: return "Word Document (.docx)"
        case .doc: return "Word Document (.doc)"
        }
    }

    static func from(url: URL) -> DocumentFormat? {
        switch url.pathExtension.lowercased() {
        case "pdf": return .pdf
        case "docx": return .docx
        case "doc": return .doc
        default: return nil
        }
    }
}

/// Job configuration for document import
struct DocumentImportJob {
    let fileURL: URL
    let format: DocumentFormat
    var outputName: String

    init(fileURL: URL) throws {
        guard let format = DocumentFormat.from(url: fileURL) else {
            throw DocumentImportError.unsupportedFormat(fileURL.pathExtension)
        }
        self.fileURL = fileURL
        self.format = format
        self.outputName = fileURL.deletingPathExtension().lastPathComponent
    }
}

enum DocumentImportError: LocalizedError {
    case unsupportedFormat(String)
    case conversionFailed(String)
    case fileNotFound
    case emptyDocument

    var errorDescription: String? {
        switch self {
        case .unsupportedFormat(let ext):
            return "Unsupported format: .\(ext). Supported formats: PDF, DOCX, DOC"
        case .conversionFailed(let reason):
            return "Conversion failed: \(reason)"
        case .fileNotFound:
            return "File not found"
        case .emptyDocument:
            return "Document has no pages"
        }
    }
}
