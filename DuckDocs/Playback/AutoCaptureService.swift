//
//  AutoCaptureService.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import AppKit
import CoreGraphics
import os

/// Service for automatic capture workflow
@Observable
@MainActor
final class AutoCaptureService {
    enum State: Equatable {
        case idle
        case preparing
        case capturing(current: Int, total: Int)
        case processing(current: Int, total: Int)
        case saving
        case completed(URL)
        case error(String)
        case partiallyCompleted(successCount: Int, failedCount: Int)
    }

    private(set) var state: State = .idle
    private(set) var capturedImages: [NSImage] = []
    private(set) var processingResults: [ImageProcessingResult] = []
    private(set) var currentJob: CaptureJob?

    private let screenCapture = ScreenCapture()
    private let outputBuilder = DocumentationOutputBuilder()
    private var captureTask: Task<Void, Never>?

    /// Start delay before capturing begins
    var startDelay: TimeInterval = 3.0

    init() {}

    /// Run the full auto-capture workflow
    func run(job: CaptureJob, aiService: AIService) {
        guard !isBusy else { return }

        if let configurationIssue = aiService.configurationIssue {
            state = .error(configurationIssue)
            return
        }

        resetForNewJob(job: job)

        captureTask = Task {
            await executeJob(job, aiService: aiService)
        }
    }

    /// Cancel the current job
    func cancel() {
        captureTask?.cancel()
        captureTask = nil
        state = .idle
        capturedImages = []
        processingResults = []
        currentJob = nil
        NSApp.unhide(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func executeJob(_ job: CaptureJob, aiService: AIService) async {
        capturedImages = []
        processingResults = []

        // Phase 1: Prepare - show preview window with countdown
        state = .preparing

        // Show preview window during countdown
        let previewWindow = await showCapturePreview(mode: job.captureMode)

        // Wait for countdown or cancellation
        let shouldContinue = await waitForPreviewCountdown(previewWindow)
        if !shouldContinue || Task.isCancelled {
            return
        }

        // Now hide app and continue with capture
        NSApp.hide(nil)

        if Task.isCancelled { return }

        // Phase 2: Capture loop
        for i in 0..<job.captureCount {
            if Task.isCancelled { return }

            state = .capturing(current: i + 1, total: job.captureCount)

            // Take screenshot first (for the first one, capture current state)
            do {
                let image = try await capture(mode: job.captureMode)
                capturedImages.append(image)
            } catch {
                state = .error("Capture failed: \(error.localizedDescription)")
                showApp()
                return
            }

            // Perform next action (except after last capture)
            if i < job.captureCount - 1 {
                await performAction(job.nextAction)
                try? await Task.sleep(nanoseconds: UInt64(job.delayBetweenCaptures * 1_000_000_000))
            }
        }

        if Task.isCancelled { return }

        // Show app before processing
        showApp()

        // Phase 3: AI Processing (parallel)
        state = .processing(current: 0, total: capturedImages.count)

        let results = await processImagesInParallel(images: capturedImages, aiService: aiService)

        if Task.isCancelled { return }

        let failedCount = results.filter { $0.status == .failed }.count
        let successCount = results.filter { $0.status == .success }.count

        if failedCount > 0 && successCount == 0 {
            // All failed
            state = .error("All images failed to process. Check your API key and try again.")
            return
        } else if failedCount > 0 {
            // Partial failure - show partial completion state
            state = .partiallyCompleted(successCount: successCount, failedCount: failedCount)
            return
        }

        await performSave(job: job)
    }

    private func capture(mode: CaptureMode) async throws -> NSImage {
        switch mode {
        case .fullScreen:
            return try await screenCapture.captureScreen()
        case .region(let region):
            return try await screenCapture.captureRegion(region)
        case .window(let windowID, _, _):
            return try await screenCapture.captureWindowByID(windowID)
        }
    }

    private func performAction(_ action: NextAction) async {
        switch action {
        case .keyPress(let keyCode, let modifiers):
            let cgFlags = modifiers.toCGEventFlags()

            if let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(keyCode), keyDown: true) {
                keyDown.flags = cgFlags
                keyDown.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 50_000_000) // 50ms

            if let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(keyCode), keyDown: false) {
                keyUp.flags = cgFlags
                keyUp.post(tap: .cghidEventTap)
            }

        case .click(let x, let y):
            let point = CGPoint(x: x, y: y)

            if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                       mouseCursorPosition: point, mouseButton: .left) {
                moveEvent.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 50_000_000)

            if let downEvent = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown,
                                       mouseCursorPosition: point, mouseButton: .left) {
                downEvent.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 50_000_000)

            if let upEvent = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp,
                                     mouseCursorPosition: point, mouseButton: .left) {
                upEvent.post(tap: .cghidEventTap)
            }

        case .none:
            break
        }
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

    private func showApp() {
        NSApp.unhide(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func showCapturePreview(mode: CaptureMode) async -> CapturePreviewWindow? {
        // Take a quick preview screenshot
        let previewImage: NSImage
        do {
            previewImage = try await capture(mode: mode)
        } catch {
            // If preview fails, continue without preview
            return nil
        }

        let window = CapturePreviewWindow(previewImage: previewImage, captureMode: mode)
        window.show()
        return window
    }

    private func waitForPreviewCountdown(_ window: CapturePreviewWindow?) async -> Bool {
        guard let window = window else {
            // No preview, just wait the normal delay
            try? await Task.sleep(nanoseconds: UInt64(startDelay * 1_000_000_000))
            return true
        }

        return await withCheckedContinuation { continuation in
            let hasResumed = OSAllocatedUnfairLock(initialState: false)

            window.onCancel = {
                hasResumed.withLock { completed in
                    guard !completed else { return }
                    completed = true
                    Task { @MainActor in
                        self.cancel()
                    }
                    continuation.resume(returning: false)
                }
            }

            window.startCountdown {
                hasResumed.withLock { completed in
                    guard !completed else { return }
                    completed = true
                    continuation.resume(returning: true)
                }
            }
        }
    }

    /// Retry failed images
    func retryFailed(aiService: AIService) {
        guard case .partiallyCompleted = state else { return }

        captureTask = Task {
            await retryFailedImages(aiService: aiService)
        }
    }

    private func retryFailedImages(aiService: AIService) async {
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
        } else if let currentJob {
            await performSave(job: currentJob)
        }
    }

    /// Save results (call after all retries done or user accepts partial)
    func saveResults() {
        guard let currentJob else { return }

        captureTask = Task {
            await performSave(job: currentJob)
        }
    }

    private func performSave(job: CaptureJob) async {
        state = .saving

        do {
            let url = try outputBuilder.exportCapture(job: job, results: processingResults)
            state = .completed(url)
        } catch {
            state = .error("Save failed: \(error.localizedDescription)")
        }
    }

    private var isBusy: Bool {
        switch state {
        case .preparing, .capturing, .processing, .saving:
            return true
        default:
            return false
        }
    }

    private func resetForNewJob(job: CaptureJob) {
        captureTask?.cancel()
        captureTask = nil
        capturedImages = []
        processingResults = []
        currentJob = job
        state = .idle
    }
}
