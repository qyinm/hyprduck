//
//  EventMonitor.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import CoreGraphics
import AppKit

/// Legacy event monitor used by the recording/playback subsystem.
final class EventMonitor: Sendable {
    /// Callback when an action is captured
    nonisolated(unsafe) var onActionCaptured: ((Action) -> Void)?

    /// Callback when monitoring fails
    nonisolated(unsafe) var onError: ((Error) -> Void)?

    /// Enable keyboard monitoring (disabled by default for privacy)
    nonisolated(unsafe) var captureKeyboard: Bool = true

    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var isRunning = false

    // For tracking drag operations
    private var dragStartPoint: CGPoint?
    private var lastEventTime: Date?

    // Click detection
    private var lastClickTime: Date?
    private var lastClickPoint: CGPoint?
    private let doubleClickInterval: TimeInterval = 0.3
    private let doubleClickRadius: CGFloat = 5.0

    // Keyboard text accumulator for batching
    private var textBuffer: String = ""
    nonisolated(unsafe) private var textBufferTask: Task<Void, Never>?
    private let textBufferDelay: Duration = .milliseconds(500)

    init() {}

    deinit {
        stop()
    }

    /// Start monitoring mouse events
    func start() throws {
        guard !isRunning else { return }

        // Check accessibility permission
        guard AXIsProcessTrusted() else {
            throw EventMonitorError.accessibilityNotGranted
        }

        // Define events to monitor (mouse + keyboard)
        var eventMask: CGEventMask =
            (1 << CGEventType.leftMouseDown.rawValue) |
            (1 << CGEventType.leftMouseUp.rawValue) |
            (1 << CGEventType.leftMouseDragged.rawValue) |
            (1 << CGEventType.rightMouseDown.rawValue) |
            (1 << CGEventType.rightMouseUp.rawValue) |
            (1 << CGEventType.scrollWheel.rawValue)

        // Add keyboard events if enabled
        if captureKeyboard {
            eventMask |= (1 << CGEventType.keyDown.rawValue)
            eventMask |= (1 << CGEventType.keyUp.rawValue)
            eventMask |= (1 << CGEventType.flagsChanged.rawValue)
        }

        // Create event tap
        let callback: CGEventTapCallBack = { proxy, type, event, refcon in
            guard let refcon = refcon else { return Unmanaged.passRetained(event) }
            let monitor = Unmanaged<EventMonitor>.fromOpaque(refcon).takeUnretainedValue()
            monitor.handleEvent(type: type, event: event)
            return Unmanaged.passRetained(event)
        }

        let refcon = Unmanaged.passUnretained(self).toOpaque()

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: eventMask,
            callback: callback,
            userInfo: refcon
        ) else {
            throw EventMonitorError.failedToCreateTap
        }

        eventTap = tap

        // Create run loop source
        runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)

        guard let source = runLoopSource else {
            throw EventMonitorError.failedToCreateRunLoopSource
        }

        // Add to run loop
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)

        isRunning = true
        lastEventTime = Date()
    }

    /// Stop monitoring mouse events
    func stop() {
        guard isRunning else { return }

        if let tap = eventTap {
            CGEvent.tapEnable(tap: tap, enable: false)
        }

        if let source = runLoopSource {
            CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, .commonModes)
        }

        eventTap = nil
        runLoopSource = nil
        isRunning = false
        dragStartPoint = nil

        // Flush any remaining text buffer
        flushTextBuffer()
        textBufferTask?.cancel()
        textBufferTask = nil
    }

    // MARK: - Event Handling

    private func handleEvent(type: CGEventType, event: CGEvent) {
        // Capture values we need before dispatching to main thread
        let location = event.location
        let eventType = type

        // Extract event-specific data on the event tap thread
        var scrollDeltaX: CGFloat = 0
        var scrollDeltaY: CGFloat = 0
        if type == .scrollWheel {
            scrollDeltaX = event.getDoubleValueField(.scrollWheelEventDeltaAxis2)
            scrollDeltaY = event.getDoubleValueField(.scrollWheelEventDeltaAxis1)
        }

        // Dispatch to main thread for all state mutations and callbacks
        DispatchQueue.main.async { [weak self] in
            self?.processEvent(
                type: eventType,
                location: location,
                event: event,
                scrollDeltaX: scrollDeltaX,
                scrollDeltaY: scrollDeltaY
            )
        }
    }

    private func processEvent(type: CGEventType, location: CGPoint, event: CGEvent, scrollDeltaX: CGFloat, scrollDeltaY: CGFloat) {
        // Calculate delay since last event (only for mouse events, not keyboard)
        let now = Date()
        let isMouseEvent = type == .leftMouseDown || type == .leftMouseUp ||
                          type == .rightMouseDown || type == .rightMouseUp ||
                          type == .scrollWheel || type == .leftMouseDragged

        if isMouseEvent {
            if let lastTime = lastEventTime {
                let delay = now.timeIntervalSince(lastTime)
                if delay > 0.5 { // Only record significant delays (500ms+)
                    onActionCaptured?(.delay(seconds: delay))
                }
            }
            lastEventTime = now
        }

        switch type {
        case .leftMouseDown:
            dragStartPoint = location

        case .leftMouseUp:
            if let startPoint = dragStartPoint {
                let distance = hypot(location.x - startPoint.x, location.y - startPoint.y)

                if distance > 10 {
                    // This was a drag
                    onActionCaptured?(.drag(
                        fromX: startPoint.x,
                        fromY: startPoint.y,
                        toX: location.x,
                        toY: location.y
                    ))
                } else {
                    // This was a click - check for double click
                    let isDoubleClick = checkForDoubleClick(at: location)

                    if isDoubleClick {
                        onActionCaptured?(.doubleClick(x: location.x, y: location.y, button: .left))
                    } else {
                        onActionCaptured?(.click(x: location.x, y: location.y, button: .left))
                    }

                    lastClickTime = now
                    lastClickPoint = location
                }
            }
            dragStartPoint = nil

        case .rightMouseDown:
            // Right click - record immediately on mouse down
            break

        case .rightMouseUp:
            onActionCaptured?(.click(x: location.x, y: location.y, button: .right))

        case .scrollWheel:
            if abs(scrollDeltaX) > 0.1 || abs(scrollDeltaY) > 0.1 {
                onActionCaptured?(.scroll(
                    x: location.x,
                    y: location.y,
                    deltaX: scrollDeltaX,
                    deltaY: scrollDeltaY
                ))
            }

        case .leftMouseDragged:
            // Dragging in progress - don't emit action yet
            break

        case .keyDown:
            handleKeyDown(event: event)

        case .keyUp:
            // Key up is handled as part of keyDown for simplicity
            break

        case .flagsChanged:
            // Modifier key change - we handle modifiers with regular keys
            break

        default:
            break
        }
    }

    // MARK: - Keyboard Event Handling

    private func handleKeyDown(event: CGEvent) {
        // Extract all needed values on the event tap thread
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        let flags = event.flags
        let modifiers = ModifierFlags.from(cgFlags: flags)

        // Get the character representation
        var character: String?

        // Try to get the Unicode string
        var unicodeLength: Int = 0
        var unicodeString = [UniChar](repeating: 0, count: 4)
        event.keyboardGetUnicodeString(maxStringLength: 4, actualStringLength: &unicodeLength, unicodeString: &unicodeString)

        if unicodeLength > 0 {
            let str = String(utf16CodeUnits: unicodeString, count: unicodeLength)
            // Filter out control characters (ASCII 0-31 except common ones)
            let filtered = str.filter { char in
                guard let ascii = char.asciiValue else { return true } // Keep non-ASCII
                return ascii >= 32 || ascii == 9 || ascii == 10 || ascii == 13 // Space+, Tab, LF, CR
            }
            if !filtered.isEmpty {
                character = filtered
            }
        }

        // Dispatch to main thread for all state mutations and callbacks
        DispatchQueue.main.async { [weak self] in
            self?.processKeyDown(keyCode: keyCode, character: character, modifiers: modifiers)
        }
    }

    private func processKeyDown(keyCode: Int64, character: String?, modifiers: ModifierFlags) {
        // Check if it's a printable character without command modifiers (shift is OK)
        let hasCommandModifiers = modifiers.contains(.command) || modifiers.contains(.control) || modifiers.contains(.option)

        // Special key codes that should be recorded as key presses, not text
        // Return(36), Tab(48), Delete(51), Escape(53), Enter(76), Arrows(123-126)
        let specialKeyCodes: Set<Int64> = [36, 48, 51, 53, 76, 123, 124, 125, 126]

        if !hasCommandModifiers && !specialKeyCodes.contains(keyCode) {
            // Regular typing - accumulate text
            if let char = character, !char.isEmpty {
                textBuffer.append(char)
                resetTextBufferTimer()
            }
            // Don't emit anything for regular typing until buffer flushes
        } else {
            // Special key or command modifier - emit as key press
            flushTextBuffer()
            // Only emit if it's a meaningful key press
            if specialKeyCodes.contains(keyCode) || hasCommandModifiers {
                onActionCaptured?(Action.keyPress(keyCode: keyCode, character: character, modifiers: modifiers))
            }
        }
    }

    private func resetTextBufferTimer() {
        textBufferTask?.cancel()
        textBufferTask = Task { [weak self] in
            try? await Task.sleep(for: self?.textBufferDelay ?? .milliseconds(500))
            guard !Task.isCancelled else { return }
            self?.flushTextBuffer()
        }
    }

    private func flushTextBuffer() {
        guard !textBuffer.isEmpty else { return }
        onActionCaptured?(Action.typeText(text: textBuffer))
        textBuffer = ""
    }

    private func checkForDoubleClick(at point: CGPoint) -> Bool {
        guard let lastTime = lastClickTime,
              let lastPoint = lastClickPoint else {
            return false
        }

        let timeDiff = Date().timeIntervalSince(lastTime)
        let distance = hypot(point.x - lastPoint.x, point.y - lastPoint.y)

        return timeDiff < doubleClickInterval && distance < doubleClickRadius
    }
}

// MARK: - Errors

enum EventMonitorError: LocalizedError {
    case accessibilityNotGranted
    case failedToCreateTap
    case failedToCreateRunLoopSource

    var errorDescription: String? {
        switch self {
        case .accessibilityNotGranted:
            return "Accessibility permission is required. Please enable it in System Settings > Privacy & Security > Accessibility."
        case .failedToCreateTap:
            return "Failed to create event tap. Make sure the app has Accessibility permission."
        case .failedToCreateRunLoopSource:
            return "Failed to create run loop source for event monitoring."
        }
    }
}
