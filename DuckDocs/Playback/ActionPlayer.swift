//
//  ActionPlayer.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import CoreGraphics
import AppKit

/// Legacy playback subsystem retained for future product exploration.
@Observable
@MainActor
final class ActionPlayer {
    /// Playback state
    enum State {
        case idle
        case playing
        case paused
    }

    /// Current state
    private(set) var state: State = .idle

    /// Current action index
    private(set) var currentIndex: Int = 0

    /// Total actions count
    private(set) var totalActions: Int = 0

    /// Progress (0.0 to 1.0)
    var progress: Double {
        guard totalActions > 0 else { return 0 }
        return min(1.0, max(0.0, Double(currentIndex) / Double(totalActions)))
    }

    /// Playback speed multiplier
    var speedMultiplier: Double = 1.0

    /// Capture mode (full screen, region, or window)
    var captureMode: CaptureMode = .fullScreen

    /// Screen capture service
    @ObservationIgnored
    private var screenCapture = ScreenCapture()

    /// Capture results
    private(set) var captureResults: [CaptureResult] = []

    /// Callback after each action
    var onActionPlayed: ((Action, Int) -> Void)?

    /// Callback when capture is taken
    var onCaptureTaken: ((CaptureResult) -> Void)?

    /// Callback when playback completes
    var onPlaybackComplete: (([CaptureResult]) -> Void)?

    /// Callback on error
    var onError: ((Error) -> Void)?

    /// Current playback task
    @ObservationIgnored
    private var playbackTask: Task<Void, Never>?

    init() {}

    /// Delay before playback starts (seconds) to allow user to switch apps
    var startDelay: TimeInterval = 3.0

    /// Play an action sequence
    func play(_ sequence: ActionSequence) {
        guard state == .idle else { return }

        state = .playing
        currentIndex = 0
        totalActions = sequence.actionCount
        captureResults = []

        playbackTask = Task {
            // Hide the app and wait for user to switch to target app
            await MainActor.run {
                NSApp.hide(nil)
            }

            // Wait for the start delay
            if startDelay > 0 {
                try? await Task.sleep(nanoseconds: UInt64(startDelay * 1_000_000_000))
            }

            // Check if cancelled during delay
            if Task.isCancelled || state == .idle {
                return
            }

            await playSequence(sequence)

            // Show the app again after playback
            await MainActor.run {
                NSApp.unhide(nil)
                NSApp.activate(ignoringOtherApps: true)
            }
        }
    }

    /// Pause playback
    func pause() {
        guard state == .playing else { return }
        state = .paused
    }

    /// Resume playback
    func resume() {
        guard state == .paused else { return }
        state = .playing
    }

    /// Stop playback
    func stop() {
        playbackTask?.cancel()
        playbackTask = nil
        state = .idle
        currentIndex = 0

        // Show the app again
        NSApp.unhide(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    // MARK: - Private

    private func playSequence(_ sequence: ActionSequence) async {
        var stepNumber = 0

        for (index, action) in sequence.actions.enumerated() {
            // Check for cancellation or pause
            while state == .paused {
                try? await Task.sleep(nanoseconds: 100_000_000) // 100ms
                if Task.isCancelled { return }
            }

            if Task.isCancelled || state == .idle {
                return
            }

            currentIndex = index

            // Execute the action
            await executeAction(action)

            // Take screenshot after non-delay actions
            if case .delay = action {
                // Don't capture for delay actions
            } else {
                stepNumber += 1

                // Small delay before capture to let UI update
                try? await Task.sleep(nanoseconds: 200_000_000) // 200ms

                do {
                    let screenshot = try await captureWithMode()
                    let result = CaptureResult(
                        action: action,
                        screenshot: screenshot,
                        stepNumber: stepNumber
                    )
                    captureResults.append(result)
                    onCaptureTaken?(result)
                } catch {
                    onError?(error)
                }
            }

            onActionPlayed?(action, index)
        }

        state = .idle
        onPlaybackComplete?(captureResults)
    }

    private func executeAction(_ action: Action) async {
        switch action {
        case .click(let x, let y, let button):
            await performClick(at: CGPoint(x: x, y: y), button: button)

        case .doubleClick(let x, let y, let button):
            await performDoubleClick(at: CGPoint(x: x, y: y), button: button)

        case .drag(let fromX, let fromY, let toX, let toY):
            await performDrag(
                from: CGPoint(x: fromX, y: fromY),
                to: CGPoint(x: toX, y: toY)
            )

        case .scroll(let x, let y, let deltaX, let deltaY):
            await performScroll(at: CGPoint(x: x, y: y), deltaX: deltaX, deltaY: deltaY)

        case .keyPress(let keyCode, _, let modifiers):
            await performKeyPress(keyCode: keyCode, modifiers: modifiers)

        case .typeText(let text):
            await performTypeText(text)

        case .delay(let seconds):
            let adjustedDelay = seconds / speedMultiplier
            try? await Task.sleep(nanoseconds: UInt64(adjustedDelay * 1_000_000_000))
        }
    }

    // MARK: - Capture

    private func captureWithMode() async throws -> NSImage {
        switch captureMode {
        case .fullScreen:
            return try await screenCapture.captureScreen()
        case .region(let rect):
            return try await screenCapture.captureRegion(rect)
        case .window(let windowID, _, _):
            return try await screenCapture.captureWindowByID(windowID)
        }
    }

    // MARK: - Event Generation

    private func performClick(at point: CGPoint, button: MouseButton) async {
        let mouseButton: CGMouseButton = button == .left ? .left : .right
        let mouseDown: CGEventType = button == .left ? .leftMouseDown : .rightMouseDown
        let mouseUp: CGEventType = button == .left ? .leftMouseUp : .rightMouseUp

        // Move mouse to position
        if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                   mouseCursorPosition: point, mouseButton: mouseButton) {
            moveEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000) // 50ms

        // Mouse down
        if let downEvent = CGEvent(mouseEventSource: nil, mouseType: mouseDown,
                                   mouseCursorPosition: point, mouseButton: mouseButton) {
            downEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000) // 50ms

        // Mouse up
        if let upEvent = CGEvent(mouseEventSource: nil, mouseType: mouseUp,
                                 mouseCursorPosition: point, mouseButton: mouseButton) {
            upEvent.post(tap: .cghidEventTap)
        }
    }

    private func performDoubleClick(at point: CGPoint, button: MouseButton) async {
        let mouseButton: CGMouseButton = button == .left ? .left : .right
        let mouseDown: CGEventType = button == .left ? .leftMouseDown : .rightMouseDown
        let mouseUp: CGEventType = button == .left ? .leftMouseUp : .rightMouseUp

        // Move mouse
        if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                   mouseCursorPosition: point, mouseButton: mouseButton) {
            moveEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000)

        // First click
        if let downEvent = CGEvent(mouseEventSource: nil, mouseType: mouseDown,
                                   mouseCursorPosition: point, mouseButton: mouseButton) {
            downEvent.setIntegerValueField(.mouseEventClickState, value: 1)
            downEvent.post(tap: .cghidEventTap)
        }
        if let upEvent = CGEvent(mouseEventSource: nil, mouseType: mouseUp,
                                 mouseCursorPosition: point, mouseButton: mouseButton) {
            upEvent.setIntegerValueField(.mouseEventClickState, value: 1)
            upEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000)

        // Second click
        if let downEvent = CGEvent(mouseEventSource: nil, mouseType: mouseDown,
                                   mouseCursorPosition: point, mouseButton: mouseButton) {
            downEvent.setIntegerValueField(.mouseEventClickState, value: 2)
            downEvent.post(tap: .cghidEventTap)
        }
        if let upEvent = CGEvent(mouseEventSource: nil, mouseType: mouseUp,
                                 mouseCursorPosition: point, mouseButton: mouseButton) {
            upEvent.setIntegerValueField(.mouseEventClickState, value: 2)
            upEvent.post(tap: .cghidEventTap)
        }
    }

    private func performDrag(from: CGPoint, to: CGPoint) async {
        // Move to start
        if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                   mouseCursorPosition: from, mouseButton: .left) {
            moveEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000)

        // Mouse down
        if let downEvent = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown,
                                   mouseCursorPosition: from, mouseButton: .left) {
            downEvent.post(tap: .cghidEventTap)
        }

        // Drag in steps for smooth movement
        let steps = 20
        for i in 1...steps {
            let progress = Double(i) / Double(steps)
            let x = from.x + (to.x - from.x) * progress
            let y = from.y + (to.y - from.y) * progress
            let point = CGPoint(x: x, y: y)

            if let dragEvent = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDragged,
                                       mouseCursorPosition: point, mouseButton: .left) {
                dragEvent.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 10_000_000) // 10ms between steps
        }

        // Mouse up
        if let upEvent = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp,
                                 mouseCursorPosition: to, mouseButton: .left) {
            upEvent.post(tap: .cghidEventTap)
        }
    }

    private func performScroll(at point: CGPoint, deltaX: Double, deltaY: Double) async {
        // Move to position first
        if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                   mouseCursorPosition: point, mouseButton: .left) {
            moveEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 50_000_000)

        // Scroll event
        if let scrollEvent = CGEvent(scrollWheelEvent2Source: nil,
                                     units: .pixel,
                                     wheelCount: 2,
                                     wheel1: Int32(deltaY * 10),
                                     wheel2: Int32(deltaX * 10),
                                     wheel3: 0) {
            scrollEvent.post(tap: .cghidEventTap)
        }
    }

    // MARK: - Keyboard Event Generation

    private func performKeyPress(keyCode: Int64, modifiers: ModifierFlags) async {
        let cgFlags = modifiers.toCGEventFlags()

        // Key down
        if let keyDownEvent = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(keyCode), keyDown: true) {
            keyDownEvent.flags = cgFlags
            keyDownEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 30_000_000) // 30ms

        // Key up
        if let keyUpEvent = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(keyCode), keyDown: false) {
            keyUpEvent.flags = cgFlags
            keyUpEvent.post(tap: .cghidEventTap)
        }

        try? await Task.sleep(nanoseconds: 30_000_000) // 30ms
    }

    private func performTypeText(_ text: String) async {
        for character in text {
            // Use CGEvent to type each character via Unicode input
            let string = String(character)
            let utf16 = Array(string.utf16)

            if let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true) {
                event.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: utf16)
                event.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 20_000_000) // 20ms

            if let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) {
                event.post(tap: .cghidEventTap)
            }

            try? await Task.sleep(nanoseconds: 20_000_000) // 20ms between characters
        }
    }
}
