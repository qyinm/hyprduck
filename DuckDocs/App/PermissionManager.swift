//
//  PermissionManager.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import AppKit
import ScreenCaptureKit

/// Manages system permissions required by DuckDocs
@Observable
@MainActor
final class PermissionManager {
    /// Accessibility permission status
    private(set) var accessibilityGranted: Bool = false

    /// Screen capture permission status
    private(set) var screenCaptureGranted: Bool = false

    /// Whether all required permissions are granted
    var allPermissionsGranted: Bool {
        accessibilityGranted && screenCaptureGranted
    }

    init() {
        Task {
            await checkAllPermissions()
        }
    }

    /// Check all permissions
    func checkAllPermissions() async {
        checkAccessibilityPermission()
        await checkScreenCapturePermission()
    }

    // MARK: - Accessibility Permission

    /// Check if accessibility permission is granted
    func checkAccessibilityPermission() {
        accessibilityGranted = AXIsProcessTrusted()
    }

    /// Request accessibility permission (opens System Settings)
    func requestAccessibilityPermission() {
        // Try to show system prompt
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        let trusted = AXIsProcessTrustedWithOptions(options)

        // If not trusted, also open System Settings directly as fallback
        if !trusted {
            openAccessibilitySettings()
        }

        // Start polling for permission change
        startAccessibilityPolling()
    }

    private var pollingTask: Task<Void, Never>?

    private func startAccessibilityPolling() {
        pollingTask?.cancel()
        pollingTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                self.checkAccessibilityPermission()
                if self.accessibilityGranted {
                    return  // Stop polling once granted
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private func stopAccessibilityPolling() {
        pollingTask?.cancel()
        pollingTask = nil
    }

    // MARK: - Screen Capture Permission

    /// Check if screen capture permission is granted
    func checkScreenCapturePermission() async {
        do {
            // Attempting to get shareable content will trigger permission prompt if needed
            _ = try await SCShareableContent.current
            screenCaptureGranted = true
        } catch {
            screenCaptureGranted = false
        }
    }

    /// Request screen capture permission
    func requestScreenCapturePermission() {
        Task {
            await checkScreenCapturePermission()

            if !screenCaptureGranted {
                // Open System Settings to Screen Recording
                openScreenRecordingSettings()
            }
        }
    }

    /// Open System Settings to Screen Recording section
    func openScreenRecordingSettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
            NSWorkspace.shared.open(url)
        }
    }

    /// Open System Settings to Accessibility section
    func openAccessibilitySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            NSWorkspace.shared.open(url)
        }
    }
}

// MARK: - Permission Status

extension PermissionManager {
    enum PermissionStatus {
        case granted
        case denied
        case unknown
    }

    var accessibilityStatus: PermissionStatus {
        accessibilityGranted ? .granted : .denied
    }

    var screenCaptureStatus: PermissionStatus {
        screenCaptureGranted ? .granted : .denied
    }
}
