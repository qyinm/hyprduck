//
//  DocumentImportService.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-03.
//

import Foundation
import AppKit
import os

/// Service for importing documents and converting to markdown
@Observable
@MainActor
final class DocumentImportService {
    private static let logger = Logger(subsystem: "com.duckdocs", category: "DocumentImportService")

    enum State: Equatable {
        case idle
        case converting
        case processing(current: Int, total: Int)
        case saving
        case completed(URL)
        case error(String)
        case partiallyCompleted(successCount: Int, failedCount: Int)
    }

    private(set) var state: State = .idle
    private(set) var pageImages: [NSImage] = []
    private(set) var processingResults: [ImageProcessingResult] = []
    private(set) var currentJob: DocumentImportJob?
    private(set) var lastParseResult: SchemaParseResult?

    private let outputBuilder = DocumentationOutputBuilder()
    private var importTask: Task<Void, Never>?
    private var currentEngine: DocumentParsingEngine?
    private var currentRequest: SchemaParseRequest?
    private var startedAt: Date?

    init() {}

    /// Import a document file and convert to markdown
    func run(job: DocumentImportJob, aiService: AIService) {
        let request = SchemaParseRequest(job: job, template: aiService.selectedTemplate)
        let engine = defaultEngine()
        run(job: job, request: request, engine: engine)
    }

    /// Import a document file using the schema-first engine seam.
    func run(job: DocumentImportJob, request: SchemaParseRequest, engine: DocumentParsingEngine) {
        guard !isBusy else { return }

        reset()
        currentJob = job
        currentRequest = request
        currentEngine = engine
        startedAt = Date()
        importTask = Task {
            await executeImport(job, request: request, engine: engine)
        }
    }

    /// Cancel the current import
    func cancel() {
        currentEngine?.cancelCurrentRun()
        importTask?.cancel()
        importTask = nil
        state = .idle
        pageImages = []
        processingResults = []
        currentJob = nil
        currentRequest = nil
        currentEngine = nil
        lastParseResult = nil
        startedAt = nil
    }

    private func executeImport(_ job: DocumentImportJob, request: SchemaParseRequest, engine: DocumentParsingEngine) async {
        Self.logger.info("Starting import: \(job.fileURL.lastPathComponent, privacy: .public)")
        state = .converting
        do {
            let result = try await parseDocument(request: request, with: engine)

            if Task.isCancelled { return }

            apply(result: result)

            if result.failedCount > 0 && result.successCount == 0 {
                state = .error("All pages failed to process. Check provider configuration and try again.")
                return
            }

            if result.failedCount > 0 {
                state = .partiallyCompleted(successCount: result.successCount, failedCount: result.failedCount)
                return
            }

            await performSave(job: job)
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    /// Retry failed pages
    func retryFailed(aiService: AIService) {
        currentEngine = defaultEngine()
        if let job = currentJob {
            currentRequest = SchemaParseRequest(job: job, template: aiService.selectedTemplate)
        }
        retryFailed()
    }

    /// Retry failed pages using the currently configured engine seam.
    func retryFailed() {
        guard case .partiallyCompleted = state else { return }
        guard currentEngine != nil, currentRequest != nil else {
            state = .error("Retry failed because the parsing engine is no longer available.")
            return
        }

        importTask = Task {
            await retryFailedPages()
        }
    }

    private func retryFailedPages() async {
        guard let engine = currentEngine, let request = currentRequest else {
            state = .error("Retry failed because the parsing engine is no longer available.")
            return
        }
        do {
            let result = try await parseDocument(request: request, with: engine)
            apply(result: result)

            if result.failedCount > 0 {
                state = .partiallyCompleted(successCount: result.successCount, failedCount: result.failedCount)
            } else if let job = currentJob {
                await performSave(job: job)
            }
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    /// Save results
    func saveResults() {
        guard let job = currentJob else { return }

        importTask = Task {
            await performSave(job: job)
        }
    }

    private func performSave(job: DocumentImportJob) async {
        state = .saving

        do {
            let parseResult = lastParseResult ?? buildParseResult(engineID: currentEngine?.engineID ?? "legacy-image-processing")
            lastParseResult = parseResult
            let url = try outputBuilder.exportImport(job: job, parseResult: parseResult, images: pageImages)
            state = .completed(url)
        } catch {
            state = .error("Save failed: \(error.localizedDescription)")
        }
    }

    /// Reset to idle state
    func reset() {
        cancel()
    }

    private func parseDocument(
        request: SchemaParseRequest,
        with engine: DocumentParsingEngine
    ) async throws -> SchemaParseResult {
        let service = self
        return try await engine.parseDocument(request: request) { event in
            Task { @MainActor in
                service.apply(event: event)
            }
        }
    }

    private func buildParseResult(engineID: String) -> SchemaParseResult {
        let pages = processingResults.sorted { $0.id < $1.id }.map { result in
            SchemaParsedPage(
                index: result.id,
                markdown: result.analysis,
                plainText: result.analysis,
                svg: nil,
                imageAssetPath: "images/page_\(result.id + 1).png",
                errorMessage: result.errorMessage
            )
        }
        let sections = pages.enumerated().map { index, page in
            MarkdownGenerator.Section(
                title: "Page \(index + 1)",
                detail: "**Source:** \(currentJob?.format.displayName ?? "Document")",
                imagePath: "images/page_\(index + 1).png",
                body: page.markdown ?? page.plainText ?? "_AI analysis unavailable for page \(index + 1)._"
            )
        }
        let markdown = MarkdownGenerator().generate(
            title: currentJob?.outputName ?? "DuckDocs Import",
            sections: sections
        )
        let duration = max(0, Int((Date().timeIntervalSince(startedAt ?? Date())) * 1000))
        let metadata = SchemaParseMetadata(
            engineID: engineID,
            durationMilliseconds: duration,
            pageCount: pages.count
        )
        let assets = pageImages.enumerated().compactMap { index, image -> SchemaOutputAsset? in
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
        let successCount = pages.filter { $0.errorMessage == nil }.count
        let failedCount = pages.count - successCount
        return SchemaParseResult(
            version: "1",
            markdown: markdown,
            pages: pages,
            assets: assets,
            metadata: metadata,
            successCount: successCount,
            failedCount: failedCount
        )
    }

    private var isBusy: Bool {
        switch state {
        case .converting, .processing, .saving:
            return true
        default:
            return false
        }
    }

    private func defaultEngine() -> DocumentParsingEngine {
        RustParsingEngine()
    }

    private func apply(event: SchemaProcessEvent) {
        switch event {
        case .queued, .documentOpened:
            state = .converting
        case .convertingPages(let current, let total):
            state = .processing(current: current, total: total)
        case .parsing(let current, let total):
            state = .processing(current: current, total: total)
        case .packaging:
            state = .saving
        case .completed:
            break
        case .failed(let message):
            state = .error(message)
        }
    }

    private func apply(result: SchemaParseResult) {
        lastParseResult = result
        pageImages = decodeImages(from: result.assets)
        processingResults = buildProcessingResults(from: result, images: pageImages)
    }

    private func buildProcessingResults(from result: SchemaParseResult, images: [NSImage]) -> [ImageProcessingResult] {
        result.pages.enumerated().map { index, page in
            let image = images.indices.contains(index) ? images[index] : NSImage(size: NSSize(width: 1, height: 1))
            let status: ImageProcessingResult.Status = page.errorMessage == nil ? .success : .failed
            return ImageProcessingResult(
                id: page.index,
                image: image,
                status: status,
                analysis: page.markdown ?? page.plainText,
                errorMessage: page.errorMessage
            )
        }
    }

    private func decodeImages(from assets: [SchemaOutputAsset]) -> [NSImage] {
        assets.compactMap { asset in
            guard asset.mimeType == "image/png",
                  let data = Data(base64Encoded: asset.base64) else {
                return nil
            }
            return NSImage(data: data)
        }
    }
}
