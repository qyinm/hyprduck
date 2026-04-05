//
//  CaptureJob.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import CoreGraphics

/// Action to perform between captures
enum NextAction: Equatable {
    case keyPress(keyCode: Int64, modifiers: ModifierFlags)
    case click(x: CGFloat, y: CGFloat)
    case none

    var displayName: String {
        switch self {
        case .keyPress(let keyCode, let modifiers):
            var parts: [String] = []
            if modifiers.contains(.command) { parts.append("⌘") }
            if modifiers.contains(.option) { parts.append("⌥") }
            if modifiers.contains(.control) { parts.append("⌃") }
            if modifiers.contains(.shift) { parts.append("⇧") }
            parts.append(keyCodeToString(keyCode))
            return parts.joined()
        case .click(let x, let y):
            return "Click (\(Int(x)), \(Int(y)))"
        case .none:
            return "None"
        }
    }

    private func keyCodeToString(_ keyCode: Int64) -> String {
        switch keyCode {
        case 123: return "←"
        case 124: return "→"
        case 125: return "↓"
        case 126: return "↑"
        case 36: return "↵"
        case 49: return "Space"
        case 48: return "Tab"
        case 53: return "Esc"
        case 51: return "Delete"
        default: return "Key\(keyCode)"
        }
    }
}

/// Settings for an auto-capture job
struct CaptureJob {
    var captureMode: CaptureMode = .fullScreen
    var nextAction: NextAction = .keyPress(keyCode: 124, modifiers: []) // Right arrow
    var captureCount: Int = 5
    var delayBetweenCaptures: TimeInterval = 0.5
    var outputName: String = "Documentation"
}
