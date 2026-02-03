//
//  QuickEntryWindow.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-01.
//

import AppKit
import SwiftUI

/// A floating Quick Entry window like Claude Desktop's Quick Entry
class QuickEntryWindow: NSPanel {
    static var shared: QuickEntryWindow?

    private var hostingView: NSHostingView<QuickEntryView>?

    var onStartCapture: ((CaptureMode) -> Void)?
    var onDismiss: (() -> Void)?

    init() {
        let windowSize = CGSize(width: 400, height: 200)

        // Center on screen
        let screenFrame = NSScreen.main?.visibleFrame ?? .zero
        let windowFrame = CGRect(
            x: screenFrame.midX - windowSize.width / 2,
            y: screenFrame.midY - windowSize.height / 2 + 100, // Slightly above center
            width: windowSize.width,
            height: windowSize.height
        )

        super.init(
            contentRect: windowFrame,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )

        // Configure panel
        level = .floating
        isFloatingPanel = true
        hidesOnDeactivate = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true

        // Allow key events
        becomesKeyOnlyIfNeeded = false

        setupUI()
    }

    private func setupUI() {
        let quickEntryView = QuickEntryView(
            onStartCapture: { [weak self] mode in
                self?.onStartCapture?(mode)
                self?.dismiss()
            },
            onDismiss: { [weak self] in
                self?.dismiss()
            }
        )

        hostingView = NSHostingView(rootView: quickEntryView)
        hostingView?.frame = contentView?.bounds ?? .zero
        hostingView?.autoresizingMask = [.width, .height]

        contentView = hostingView
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    func show() {
        // Re-center on current screen
        if let screen = NSScreen.main {
            let screenFrame = screen.visibleFrame
            let windowSize = frame.size
            let newOrigin = CGPoint(
                x: screenFrame.midX - windowSize.width / 2,
                y: screenFrame.midY - windowSize.height / 2 + 100
            )
            setFrameOrigin(newOrigin)
        }

        makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        QuickEntryWindow.shared = self
    }

    func dismiss() {
        orderOut(nil)
        onDismiss?()
        QuickEntryWindow.shared = nil
    }

    // Handle ESC key to dismiss
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { // ESC
            dismiss()
        } else {
            super.keyDown(with: event)
        }
    }
}

// MARK: - SwiftUI View

struct QuickEntryView: View {
    let onStartCapture: (CaptureMode) -> Void
    let onDismiss: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            // Header
            HStack {
                Image(systemName: "camera.viewfinder")
                    .font(.title2)
                    .foregroundStyle(.secondary)

                Text("Quick Capture")
                    .font(.headline)

                Spacer()

                Button(action: onDismiss) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .keyboardShortcut(.escape, modifiers: [])
            }

            Divider()

            // Capture options
            HStack(spacing: 12) {
                QuickCaptureButton(
                    icon: "rectangle.dashed",
                    title: "Full Screen",
                    shortcut: "1"
                ) {
                    onStartCapture(.fullScreen)
                }

                QuickCaptureButton(
                    icon: "rectangle.dashed.badge.record",
                    title: "Region",
                    shortcut: "2"
                ) {
                    // For region, we need to dismiss first then show selector
                    onDismiss()
                    Task { @MainActor in
                        try? await Task.sleep(for: .milliseconds(100))
                        NotificationCenter.default.post(name: .quickEntrySelectRegion, object: nil)
                    }
                }

                QuickCaptureButton(
                    icon: "macwindow",
                    title: "Window",
                    shortcut: "3"
                ) {
                    onDismiss()
                    Task { @MainActor in
                        try? await Task.sleep(for: .milliseconds(100))
                        NotificationCenter.default.post(name: .quickEntrySelectWindow, object: nil)
                    }
                }
            }

            // Hint
            Text("Press 1, 2, or 3 to select • ESC to dismiss")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(20)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(.ultraThinMaterial)
                .shadow(color: .black.opacity(0.2), radius: 20, x: 0, y: 10)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(Color.primary.opacity(0.1), lineWidth: 1)
        )
    }
}

struct QuickCaptureButton: View {
    let icon: String
    let title: String
    let shortcut: String
    let action: () -> Void

    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            VStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.system(size: 28))
                    .frame(height: 32)

                Text(title)
                    .font(.caption)
                    .fontWeight(.medium)

                Text(shortcut)
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.2))
                    .cornerRadius(4)
            }
            .frame(width: 100, height: 90)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    .fill(isHovered ? Color.accentColor.opacity(0.15) : Color.clear)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isHovered ? Color.accentColor : Color.clear, lineWidth: 2)
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            isHovered = hovering
        }
        .keyboardShortcut(KeyEquivalent(Character(shortcut)), modifiers: [])
    }
}

// MARK: - Notification Names

extension Notification.Name {
    static let quickEntrySelectRegion = Notification.Name("quickEntrySelectRegion")
    static let quickEntrySelectWindow = Notification.Name("quickEntrySelectWindow")
}
