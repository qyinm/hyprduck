//
//  Action.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import CoreGraphics

/// Mouse button types
enum MouseButton: String, Codable {
    case left
    case right
    case center
}

/// Scroll direction
enum ScrollDirection: String, Codable {
    case up
    case down
    case left
    case right
}

/// Modifier flags for keyboard events
struct ModifierFlags: OptionSet, Codable, Equatable {
    let rawValue: UInt64

    static let shift = ModifierFlags(rawValue: 1 << 0)
    static let control = ModifierFlags(rawValue: 1 << 1)
    static let option = ModifierFlags(rawValue: 1 << 2)
    static let command = ModifierFlags(rawValue: 1 << 3)
    static let capsLock = ModifierFlags(rawValue: 1 << 4)
    static let function = ModifierFlags(rawValue: 1 << 5)

    static func from(cgFlags: CGEventFlags) -> ModifierFlags {
        var flags = ModifierFlags()
        if cgFlags.contains(.maskShift) { flags.insert(.shift) }
        if cgFlags.contains(.maskControl) { flags.insert(.control) }
        if cgFlags.contains(.maskAlternate) { flags.insert(.option) }
        if cgFlags.contains(.maskCommand) { flags.insert(.command) }
        if cgFlags.contains(.maskAlphaShift) { flags.insert(.capsLock) }
        if cgFlags.contains(.maskSecondaryFn) { flags.insert(.function) }
        return flags
    }

    func toCGEventFlags() -> CGEventFlags {
        var flags = CGEventFlags()
        if contains(.shift) { flags.insert(.maskShift) }
        if contains(.control) { flags.insert(.maskControl) }
        if contains(.option) { flags.insert(.maskAlternate) }
        if contains(.command) { flags.insert(.maskCommand) }
        if contains(.capsLock) { flags.insert(.maskAlphaShift) }
        if contains(.function) { flags.insert(.maskSecondaryFn) }
        return flags
    }
}

/// Represents a single user action that can be recorded and replayed
enum Action: Codable, Equatable {
    /// Mouse click at screen coordinates
    case click(x: CGFloat, y: CGFloat, button: MouseButton)

    /// Mouse double click at screen coordinates
    case doubleClick(x: CGFloat, y: CGFloat, button: MouseButton)

    /// Mouse drag from one point to another
    case drag(fromX: CGFloat, fromY: CGFloat, toX: CGFloat, toY: CGFloat)

    /// Scroll wheel event
    case scroll(x: CGFloat, y: CGFloat, deltaX: CGFloat, deltaY: CGFloat)

    /// Key press (down + up)
    case keyPress(keyCode: Int64, character: String?, modifiers: ModifierFlags)

    /// Text input (for typing strings)
    case typeText(text: String)

    /// Delay between actions
    case delay(seconds: Double)

    // MARK: - Codable

    private enum CodingKeys: String, CodingKey {
        case type
        case x, y, button
        case fromX, fromY, toX, toY
        case deltaX, deltaY
        case keyCode, character, modifiers
        case text
        case seconds
    }

    private enum ActionType: String, Codable {
        case click
        case doubleClick
        case drag
        case scroll
        case keyPress
        case typeText
        case delay
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(ActionType.self, forKey: .type)

        switch type {
        case .click:
            let x = try container.decode(CGFloat.self, forKey: .x)
            let y = try container.decode(CGFloat.self, forKey: .y)
            let button = try container.decode(MouseButton.self, forKey: .button)
            self = .click(x: x, y: y, button: button)

        case .doubleClick:
            let x = try container.decode(CGFloat.self, forKey: .x)
            let y = try container.decode(CGFloat.self, forKey: .y)
            let button = try container.decode(MouseButton.self, forKey: .button)
            self = .doubleClick(x: x, y: y, button: button)

        case .drag:
            let fromX = try container.decode(CGFloat.self, forKey: .fromX)
            let fromY = try container.decode(CGFloat.self, forKey: .fromY)
            let toX = try container.decode(CGFloat.self, forKey: .toX)
            let toY = try container.decode(CGFloat.self, forKey: .toY)
            self = .drag(fromX: fromX, fromY: fromY, toX: toX, toY: toY)

        case .scroll:
            let x = try container.decode(CGFloat.self, forKey: .x)
            let y = try container.decode(CGFloat.self, forKey: .y)
            let deltaX = try container.decode(CGFloat.self, forKey: .deltaX)
            let deltaY = try container.decode(CGFloat.self, forKey: .deltaY)
            self = .scroll(x: x, y: y, deltaX: deltaX, deltaY: deltaY)

        case .keyPress:
            let keyCode = try container.decode(Int64.self, forKey: .keyCode)
            let character = try container.decodeIfPresent(String.self, forKey: .character)
            let modifiers = try container.decode(ModifierFlags.self, forKey: .modifiers)
            self = .keyPress(keyCode: keyCode, character: character, modifiers: modifiers)

        case .typeText:
            let text = try container.decode(String.self, forKey: .text)
            self = .typeText(text: text)

        case .delay:
            let seconds = try container.decode(Double.self, forKey: .seconds)
            self = .delay(seconds: seconds)
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)

        switch self {
        case .click(let x, let y, let button):
            try container.encode(ActionType.click, forKey: .type)
            try container.encode(x, forKey: .x)
            try container.encode(y, forKey: .y)
            try container.encode(button, forKey: .button)

        case .doubleClick(let x, let y, let button):
            try container.encode(ActionType.doubleClick, forKey: .type)
            try container.encode(x, forKey: .x)
            try container.encode(y, forKey: .y)
            try container.encode(button, forKey: .button)

        case .drag(let fromX, let fromY, let toX, let toY):
            try container.encode(ActionType.drag, forKey: .type)
            try container.encode(fromX, forKey: .fromX)
            try container.encode(fromY, forKey: .fromY)
            try container.encode(toX, forKey: .toX)
            try container.encode(toY, forKey: .toY)

        case .scroll(let x, let y, let deltaX, let deltaY):
            try container.encode(ActionType.scroll, forKey: .type)
            try container.encode(x, forKey: .x)
            try container.encode(y, forKey: .y)
            try container.encode(deltaX, forKey: .deltaX)
            try container.encode(deltaY, forKey: .deltaY)

        case .keyPress(let keyCode, let character, let modifiers):
            try container.encode(ActionType.keyPress, forKey: .type)
            try container.encode(keyCode, forKey: .keyCode)
            try container.encodeIfPresent(character, forKey: .character)
            try container.encode(modifiers, forKey: .modifiers)

        case .typeText(let text):
            try container.encode(ActionType.typeText, forKey: .type)
            try container.encode(text, forKey: .text)

        case .delay(let seconds):
            try container.encode(ActionType.delay, forKey: .type)
            try container.encode(seconds, forKey: .seconds)
        }
    }
}

// MARK: - Action Description

extension Action: CustomStringConvertible {
    var description: String {
        switch self {
        case .click(let x, let y, let button):
            return "Click (\(button)) at (\(Int(x)), \(Int(y)))"
        case .doubleClick(let x, let y, let button):
            return "Double-click (\(button)) at (\(Int(x)), \(Int(y)))"
        case .drag(let fromX, let fromY, let toX, let toY):
            return "Drag from (\(Int(fromX)), \(Int(fromY))) to (\(Int(toX)), \(Int(toY)))"
        case .scroll(let x, let y, let deltaX, let deltaY):
            return "Scroll at (\(Int(x)), \(Int(y))) delta: (\(deltaX), \(deltaY))"
        case .keyPress(let keyCode, let character, let modifiers):
            let char = character ?? "key:\(keyCode)"
            let mods = modifiers.isEmpty ? "" : " [\(modifiers.description)]"
            return "Key '\(char)'\(mods)"
        case .typeText(let text):
            let preview = text.count > 20 ? String(text.prefix(20)) + "..." : text
            return "Type \"\(preview)\""
        case .delay(let seconds):
            return "Delay \(String(format: "%.2f", seconds))s"
        }
    }
}

// MARK: - ModifierFlags Description

extension ModifierFlags: CustomStringConvertible {
    var description: String {
        var parts: [String] = []
        if contains(.command) { parts.append("⌘") }
        if contains(.shift) { parts.append("⇧") }
        if contains(.option) { parts.append("⌥") }
        if contains(.control) { parts.append("⌃") }
        if contains(.function) { parts.append("fn") }
        return parts.joined()
    }
}
