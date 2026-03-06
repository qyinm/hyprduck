//
//  CaptureSettings.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import CoreGraphics

/// A selected region on a specific display.
struct CaptureRegion: Equatable {
    var rect: CGRect
    var displayID: CGDirectDisplayID
    var displayName: String
}

/// Capture mode for documentation capture workflows.
enum CaptureMode: Equatable {
    case fullScreen
    case region(CaptureRegion)
    case window(windowID: CGWindowID, title: String, appName: String)

    var displayName: String {
        switch self {
        case .fullScreen:
            return "Full Screen"
        case .region(let region):
            return "\(region.displayName) Region (\(Int(region.rect.width))x\(Int(region.rect.height)))"
        case .window(_, let title, let appName):
            if title.isEmpty {
                return appName
            }
            return "\(appName) - \(title)"
        }
    }

    var icon: String {
        switch self {
        case .fullScreen:
            return "rectangle.dashed"
        case .region:
            return "rectangle.dashed.badge.record"
        case .window:
            return "macwindow"
        }
    }
}

/// Settings for the capture workflow.
struct CaptureSettings {
    var mode: CaptureMode = .fullScreen
}
