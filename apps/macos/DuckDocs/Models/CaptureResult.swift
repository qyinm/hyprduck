//
//  CaptureResult.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import AppKit
import CoreGraphics

/// Result of capturing a screenshot after an action
struct CaptureResult: Identifiable {
    /// Unique identifier
    let id: UUID

    /// The action that was performed before this capture
    let action: Action

    /// The captured screenshot
    let screenshot: NSImage

    /// When the capture was taken
    let timestamp: Date

    /// Step number in the sequence (1-based)
    let stepNumber: Int

    init(
        id: UUID = UUID(),
        action: Action,
        screenshot: NSImage,
        timestamp: Date = Date(),
        stepNumber: Int
    ) {
        self.id = id
        self.action = action
        self.screenshot = screenshot
        self.timestamp = timestamp
        self.stepNumber = stepNumber
    }
}

// MARK: - Export

extension CaptureResult {
    /// Save screenshot to a file
    func saveScreenshot(to directory: URL, filename: String? = nil) throws -> URL {
        let name = filename ?? "step_\(stepNumber).png"
        let url = directory.appendingPathComponent(name)

        guard let tiffData = screenshot.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiffData),
              let pngData = bitmap.representation(using: .png, properties: [:]) else {
            throw CaptureError.imageConversionFailed
        }

        try pngData.write(to: url)
        return url
    }
}

/// Errors related to capture operations
enum CaptureError: LocalizedError {
    case noDisplay
    case captureNotAllowed
    case captureFailed(String)
    case imageConversionFailed
    case windowNotFound(CGWindowID)

    var errorDescription: String? {
        switch self {
        case .noDisplay:
            return "No display available for capture"
        case .captureNotAllowed:
            return "Screen capture permission not granted"
        case .captureFailed(let message):
            return "Capture failed: \(message)"
        case .imageConversionFailed:
            return "Failed to convert image to PNG"
        case .windowNotFound(let windowID):
            return "Window not found: \(windowID)"
        }
    }
}

/// Collection of capture results for a playback session
struct PlaybackSession: Identifiable {
    let id: UUID
    let sequenceId: UUID
    let sequenceName: String
    let startedAt: Date
    var completedAt: Date?
    var captures: [CaptureResult]

    init(
        id: UUID = UUID(),
        sequenceId: UUID,
        sequenceName: String,
        startedAt: Date = Date(),
        captures: [CaptureResult] = []
    ) {
        self.id = id
        self.sequenceId = sequenceId
        self.sequenceName = sequenceName
        self.startedAt = startedAt
        self.captures = captures
    }

    /// Export all screenshots to a directory
    func exportScreenshots(to directory: URL) throws -> [URL] {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)

        return try captures.map { capture in
            try capture.saveScreenshot(to: directory)
        }
    }
}
