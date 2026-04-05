//
//  RustParsingEngine.swift
//  DuckDocs
//

import Foundation

@MainActor
final class RustParsingEngine: DocumentParsingEngine {
    private let process = RustCoreProcess()

    static var isAvailable: Bool {
        (try? RustCoreProcess().resolveEngineURLForAvailabilityCheck()) != nil
    }

    var engineID: String {
        "duckdocs-engine"
    }

    func parseDocument(
        request: SchemaParseRequest,
        onEvent: @escaping @Sendable (SchemaProcessEvent) -> Void
    ) async throws -> SchemaParseResult {
        try await process.run(request: request, onEvent: onEvent)
    }

    func cancelCurrentRun() {
        process.cancel()
    }
}
