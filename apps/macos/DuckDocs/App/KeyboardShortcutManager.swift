//
//  KeyboardShortcutManager.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-01.
//

import Foundation
import AppKit
import Carbon.HIToolbox

/// Manages global keyboard shortcuts including Quick Entry
@MainActor
final class KeyboardShortcutManager {
    static let shared = KeyboardShortcutManager()

    private var globalMonitor: Any?
    private var localMonitor: Any?
    private var lastOptionPressTime: Date?
    private let doubleTapInterval: TimeInterval = 0.3

    /// Callback when Quick Entry should be shown
    var onQuickEntry: (() -> Void)?

    /// Callback to cancel current capture
    var onCancelCapture: (() -> Void)?

    private init() {}

    func start() {
        // Global monitor for Option double-tap and Option+Space
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.flagsChanged, .keyDown]) { [weak self] event in
            DispatchQueue.main.async {
                self?.handleGlobalEvent(event)
            }
        }

        // Local monitor when app is active
        // Note: Local monitor runs on main thread but we need synchronous handling
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: [.flagsChanged, .keyDown]) { [weak self] event in
            guard let self = self else { return event }
            // Since we're @MainActor and local monitor is already on main thread,
            // we can call directly without dispatch
            if self.handleLocalEvent(event) {
                return nil
            }
            return event
        }
    }

    func stop() {
        if let monitor = globalMonitor {
            NSEvent.removeMonitor(monitor)
            globalMonitor = nil
        }
        if let monitor = localMonitor {
            NSEvent.removeMonitor(monitor)
            localMonitor = nil
        }
    }

    private func handleGlobalEvent(_ event: NSEvent) {
        // Check for Option+Space
        if event.type == .keyDown && event.keyCode == 49 { // Space key
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if flags == .option {
                onQuickEntry?()
                return
            }
        }

        // Check for Option double-tap
        if event.type == .flagsChanged {
            checkOptionDoubleTap(event)
        }

        // Check for Cmd+Shift+X to cancel
        if event.type == .keyDown && event.keyCode == 7 { // X key
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if flags == [.command, .shift] {
                onCancelCapture?()
            }
        }
    }

    @discardableResult
    private func handleLocalEvent(_ event: NSEvent) -> Bool {
        // Check for Option+Space
        if event.type == .keyDown && event.keyCode == 49 { // Space key
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if flags == .option {
                onQuickEntry?()
                return true
            }
        }

        // Check for Option double-tap
        if event.type == .flagsChanged {
            if checkOptionDoubleTap(event) {
                return true
            }
        }

        // Check for Cmd+Shift+X to cancel
        if event.type == .keyDown && event.keyCode == 7 {
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if flags == [.command, .shift] {
                onCancelCapture?()
                return true
            }
        }

        return false
    }

    @discardableResult
    private func checkOptionDoubleTap(_ event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        // Option key released (was pressed, now not pressed)
        if flags.isEmpty && event.keyCode == 58 { // 58 = Option key
            let now = Date()
            if let lastPress = lastOptionPressTime,
               now.timeIntervalSince(lastPress) < doubleTapInterval {
                // Double tap detected
                lastOptionPressTime = nil
                onQuickEntry?()
                return true
            } else {
                lastOptionPressTime = now
            }
        }
        return false
    }

    deinit {
        Task { @MainActor in
            KeyboardShortcutManager.shared.stop()
        }
    }
}
