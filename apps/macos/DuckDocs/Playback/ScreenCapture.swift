//
//  ScreenCapture.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import AppKit
import ScreenCaptureKit

/// Captures screenshots using ScreenCaptureKit
final class ScreenCapture {
    /// Capture configuration
    struct Configuration {
        var captureResolution: CaptureResolution = .high
        var showCursor: Bool = true

        enum CaptureResolution {
            case low      // 1x
            case medium   // 1.5x
            case high     // 2x (Retina)

            var scaleFactor: CGFloat {
                switch self {
                case .low: return 1.0
                case .medium: return 1.5
                case .high: return 2.0
                }
            }
        }
    }

    var configuration = Configuration()

    init() {}

    /// Capture the main display
    func captureScreen() async throws -> NSImage {
        // Get shareable content
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)

        let preferredDisplayID = NSScreen.main?.displayID
        let display = content.displays.first { $0.displayID == preferredDisplayID } ?? content.displays.first

        guard let display else {
            throw CaptureError.noDisplay
        }

        return try await captureDisplay(display)
    }

    /// Capture a specific display
    func captureDisplay(_ display: SCDisplay) async throws -> NSImage {
        let filter = SCContentFilter(display: display, excludingWindows: [])

        let config = SCStreamConfiguration()
        config.width = Int(CGFloat(display.width) * configuration.captureResolution.scaleFactor)
        config.height = Int(CGFloat(display.height) * configuration.captureResolution.scaleFactor)
        config.showsCursor = configuration.showCursor
        config.pixelFormat = kCVPixelFormatType_32BGRA

        let image = try await SCScreenshotManager.captureImage(
            contentFilter: filter,
            configuration: config
        )

        return NSImage(cgImage: image, size: NSSize(width: display.width, height: display.height))
    }

    /// Capture a specific window
    func captureWindow(_ window: SCWindow) async throws -> NSImage {
        let filter = SCContentFilter(desktopIndependentWindow: window)

        let config = SCStreamConfiguration()
        config.width = Int(CGFloat(window.frame.width) * configuration.captureResolution.scaleFactor)
        config.height = Int(CGFloat(window.frame.height) * configuration.captureResolution.scaleFactor)
        config.showsCursor = configuration.showCursor
        config.pixelFormat = kCVPixelFormatType_32BGRA

        let image = try await SCScreenshotManager.captureImage(
            contentFilter: filter,
            configuration: config
        )

        return NSImage(cgImage: image, size: window.frame.size)
    }

    /// Capture a window by its ID
    func captureWindowByID(_ windowID: CGWindowID) async throws -> NSImage {
        let windows = try await getWindows()
        guard let window = windows.first(where: { $0.windowID == windowID }) else {
            throw CaptureError.windowNotFound(windowID)
        }
        return try await captureWindow(window)
    }

    /// Find a window by its ID
    func findWindow(byID windowID: CGWindowID) async throws -> SCWindow? {
        let windows = try await getWindows()
        return windows.first { $0.windowID == windowID }
    }

    /// Capture a region of the screen
    func captureRegion(_ region: CaptureRegion) async throws -> NSImage {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)

        guard let targetDisplay = content.displays.first(where: { $0.displayID == region.displayID }) else {
            throw CaptureError.noDisplay
        }

        let filter = SCContentFilter(display: targetDisplay, excludingWindows: [])

        let config = SCStreamConfiguration()
        config.sourceRect = region.rect
        config.width = Int(region.rect.width * configuration.captureResolution.scaleFactor)
        config.height = Int(region.rect.height * configuration.captureResolution.scaleFactor)
        config.showsCursor = configuration.showCursor
        config.pixelFormat = kCVPixelFormatType_32BGRA

        let image = try await SCScreenshotManager.captureImage(
            contentFilter: filter,
            configuration: config
        )

        return NSImage(cgImage: image, size: region.rect.size)
    }

    /// Get all available displays
    func getDisplays() async throws -> [SCDisplay] {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        return content.displays
    }

    /// Get all available windows
    func getWindows(includeDesktopWindows: Bool = false) async throws -> [SCWindow] {
        let content = try await SCShareableContent.excludingDesktopWindows(
            !includeDesktopWindows,
            onScreenWindowsOnly: true
        )
        return content.windows
    }

    /// Check if screen capture is allowed
    func checkPermission() async -> Bool {
        do {
            _ = try await SCShareableContent.current
            return true
        } catch {
            return false
        }
    }
}

// MARK: - Convenience Extensions

extension SCDisplay {
    var displayName: String {
        "Display \(displayID)"
    }
}

extension SCWindow {
    var displayName: String {
        if let title = title, !title.isEmpty {
            return title
        }
        if let appName = owningApplication?.applicationName {
            return appName
        }
        return "Window \(windowID)"
    }
}

private extension NSScreen {
    var displayID: CGDirectDisplayID {
        deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID ?? 0
    }
}
