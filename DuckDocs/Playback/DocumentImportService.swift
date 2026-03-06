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

    private let converter = DocumentConverter()
    private let outputBuilder = DocumentationOutputBuilder()
    private var importTask: Task<Void, Never>?

    init() {}

    /// Import a document file and convert to markdown
    func run(job: DocumentImportJob, aiService: AIService) {
        guard !isBusy else { return }

        if let configurationIssue = aiService.configurationIssue {
            state = .error(configurationIssue)
            return
        }

        reset()
        currentJob = job
        importTask = Task {
            await executeImport(job, aiService: aiService)
        }
    }

    /// Cancel the current import
    func cancel() {
        importTask?.cancel()
        importTask = nil
        state = .idle
        pageImages = []
        processingResults = []
        currentJob = nil
    }

    private func executeImport(_ job: DocumentImportJob, aiService: AIService) async {
        Self.logger.info("Starting import: \(job.fileURL.lastPathComponent, privacy: .public)")

        // Phase 1: Convert document to images
        state = .converting

        do {
            pageImages = try await converter.convert(job)
            Self.logger.info("Converted \(self.pageImages.count) pages")
        } catch {
            state = .error(error.localizedDescription)
            return
        }

        if Task.isCancelled { return }

        // Phase 2: AI Processing (parallel)
        state = .processing(current: 0, total: pageImages.count)

        let results = await processImagesInParallel(images: pageImages, aiService: aiService)

        if Task.isCancelled { return }

        let failedCount = results.filter { $0.status == .failed }.count
        let successCount = results.filter { $0.status == .success }.count

        if failedCount > 0 && successCount == 0 {
            state = .error("All pages failed to process. Check your API key and try again.")
            return
        } else if failedCount > 0 {
            state = .partiallyCompleted(successCount: successCount, failedCount: failedCount)
            return
        }

        // Phase 3: Save
        await performSave(job: job)
    }

    private func processImagesInParallel(images: [NSImage], aiService: AIService) async -> [ImageProcessingResult] {
        // Initialize results
        processingResults = images.enumerated().map { index, image in
            ImageProcessingResult(id: index, image: image, status: .pending)
        }

        let maxConcurrent = 5
        let completedCount = OSAllocatedUnfairLock(initialState: 0)

        await withTaskGroup(of: (Int, Result<String, Error>).self) { group in
            var index = 0

            // Start initial batch
            while index < min(maxConcurrent, images.count) {
                let currentIndex = index
                let image = images[currentIndex]

                await MainActor.run {
                    processingResults[currentIndex].status = .processing
                }

                group.addTask {
                    do {
                        let result = try await aiService.analyzeImage(image)
                        return (currentIndex, .success(result))
                    } catch {
                        return (currentIndex, .failure(error))
                    }
                }
                index += 1
            }

            // Process remaining
            for await (idx, result) in group {
                let current = completedCount.withLock { count -> Int in
                    count += 1
                    return count
                }

                await MainActor.run {
                    switch result {
                    case .success(let analysis):
                        processingResults[idx].status = .success
                        processingResults[idx].analysis = analysis
                    case .failure(let error):
                        processingResults[idx].status = .failed
                        processingResults[idx].errorMessage = error.localizedDescription
                    }
                    self.state = .processing(current: current, total: images.count)
                }

                if index < images.count {
                    let currentIndex = index
                    let image = images[currentIndex]

                    await MainActor.run {
                        processingResults[currentIndex].status = .processing
                    }

                    group.addTask {
                        do {
                            let result = try await aiService.analyzeImage(image)
                            return (currentIndex, .success(result))
                        } catch {
                            return (currentIndex, .failure(error))
                        }
                    }
                    index += 1
                }
            }
        }

        return processingResults
    }

    /// Retry failed pages
    func retryFailed(aiService: AIService) {
        guard case .partiallyCompleted = state else { return }

        importTask = Task {
            await retryFailedPages(aiService: aiService)
        }
    }

    private func retryFailedPages(aiService: AIService) async {
        let failedIndices = processingResults.enumerated()
            .filter { $0.element.status == .failed }
            .map { $0.offset }

        guard !failedIndices.isEmpty else { return }

        state = .processing(current: 0, total: failedIndices.count)

        var retryCount = 0
        for idx in failedIndices {
            if Task.isCancelled { return }

            processingResults[idx].status = .processing
            processingResults[idx].errorMessage = nil

            do {
                let result = try await aiService.analyzeImage(processingResults[idx].image)
                processingResults[idx].status = .success
                processingResults[idx].analysis = result
            } catch {
                processingResults[idx].status = .failed
                processingResults[idx].errorMessage = error.localizedDescription
            }

            retryCount += 1
            state = .processing(current: retryCount, total: failedIndices.count)
        }

        let stillFailed = processingResults.filter { $0.status == .failed }.count
        let successCount = processingResults.filter { $0.status == .success }.count

        if stillFailed > 0 {
            state = .partiallyCompleted(successCount: successCount, failedCount: stillFailed)
        } else if let job = currentJob {
            await performSave(job: job)
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
            let url = try outputBuilder.exportImport(job: job, results: processingResults)
            state = .completed(url)
        } catch {
            state = .error("Save failed: \(error.localizedDescription)")
        }
    }

    /// Reset to idle state
    func reset() {
        cancel()
    }

    private var isBusy: Bool {
        switch state {
        case .converting, .processing, .saving:
            return true
        default:
            return false
        }
    }
}
