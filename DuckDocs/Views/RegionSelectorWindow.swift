//
//  RegionSelectorWindow.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import AppKit
import SwiftUI

/// A transparent overlay window for selecting a screen region
class RegionSelectorWindow: NSPanel {
    private var selectionView: RegionSelectionView!
    var onRegionSelected: ((CaptureRegion) -> Void)?
    var onCancelled: (() -> Void)?

    init() {
        let unionFrame = NSScreen.screens
            .map(\.frame)
            .reduce(CGRect.null) { partialResult, frame in
                partialResult.union(frame)
            }
        let screenFrame = unionFrame.isNull
            ? CGRect(x: 0, y: 0, width: 1920, height: 1080)
            : unionFrame

        super.init(
            contentRect: screenFrame,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )

        // Configure the panel
        level = .screenSaver
        isOpaque = false
        backgroundColor = NSColor.black.withAlphaComponent(0.3)
        hasShadow = false
        ignoresMouseEvents = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        // Create the selection view
        selectionView = RegionSelectionView(frame: CGRect(origin: .zero, size: screenFrame.size))
        selectionView.onRegionSelected = { [weak self] rect in
            self?.onRegionSelected?(rect)
            self?.close()
        }
        selectionView.onCancelled = { [weak self] in
            self?.onCancelled?()
            self?.close()
        }

        contentView = selectionView
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    func show() {
        makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}

/// The view that handles drawing and mouse events for region selection
class RegionSelectionView: NSView {
    private var startPoint: NSPoint?
    private var currentPoint: NSPoint?
    private var activeScreen: NSScreen?
    var onRegionSelected: ((CaptureRegion) -> Void)?
    var onCancelled: (() -> Void)?

    override var acceptsFirstResponder: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)

        // Draw semi-transparent overlay
        NSColor.black.withAlphaComponent(0.3).setFill()
        dirtyRect.fill()

        // Draw selection rectangle if dragging
        if let start = startPoint, let current = currentPoint {
            let selectionRect = rectFromPoints(start, current)

            // Clear the selection area
            NSColor.clear.setFill()
            selectionRect.fill(using: .copy)

            // Draw border
            NSColor.systemBlue.setStroke()
            let path = NSBezierPath(rect: selectionRect)
            path.lineWidth = 2
            path.setLineDash([6, 3], count: 2, phase: 0)
            path.stroke()

            // Draw corner handles
            let handleSize: CGFloat = 8
            NSColor.white.setFill()
            let corners = [
                CGPoint(x: selectionRect.minX, y: selectionRect.minY),
                CGPoint(x: selectionRect.maxX, y: selectionRect.minY),
                CGPoint(x: selectionRect.minX, y: selectionRect.maxY),
                CGPoint(x: selectionRect.maxX, y: selectionRect.maxY)
            ]
            for corner in corners {
                let handleRect = CGRect(
                    x: corner.x - handleSize / 2,
                    y: corner.y - handleSize / 2,
                    width: handleSize,
                    height: handleSize
                )
                NSBezierPath(ovalIn: handleRect).fill()
            }

            // Draw size label
            let size = "\(Int(selectionRect.width)) x \(Int(selectionRect.height))"
            let attributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 14, weight: .medium),
                .foregroundColor: NSColor.white,
                .backgroundColor: NSColor.black.withAlphaComponent(0.7)
            ]
            let attributedString = NSAttributedString(string: " \(size) ", attributes: attributes)
            let labelSize = attributedString.size()
            let labelPoint = NSPoint(
                x: selectionRect.midX - labelSize.width / 2,
                y: selectionRect.maxY + 10
            )
            attributedString.draw(at: labelPoint)
        }

        // Draw instructions
        let instructions = "Drag to select a region. Press Enter to confirm, ESC to cancel."
        let instructionAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 16),
            .foregroundColor: NSColor.white
        ]
        let instructionString = NSAttributedString(string: instructions, attributes: instructionAttributes)
        let instructionSize = instructionString.size()
        let instructionPoint = NSPoint(
            x: bounds.midX - instructionSize.width / 2,
            y: bounds.height - 50
        )
        instructionString.draw(at: instructionPoint)
    }

    override func mouseDown(with event: NSEvent) {
        startPoint = convert(event.locationInWindow, from: nil)
        currentPoint = startPoint
        activeScreen = screenContainingSelectionStart()
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        currentPoint = convert(event.locationInWindow, from: nil)
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        currentPoint = convert(event.locationInWindow, from: nil)
        needsDisplay = true
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53: // ESC
            onCancelled?()
        case 36: // Enter
            if let start = startPoint, let current = currentPoint {
                let rect = rectFromPoints(start, current)
                if let region = convertToCaptureRegion(rect), region.rect.width >= 10, region.rect.height >= 10 {
                    onRegionSelected?(region)
                }
            }
        default:
            super.keyDown(with: event)
        }
    }

    private func rectFromPoints(_ p1: NSPoint, _ p2: NSPoint) -> CGRect {
        CGRect(
            x: min(p1.x, p2.x),
            y: min(p1.y, p2.y),
            width: abs(p2.x - p1.x),
            height: abs(p2.y - p1.y)
        )
    }

    private func screenContainingSelectionStart() -> NSScreen? {
        guard let startPoint, let window else { return nil }
        let globalPoint = window.convertPoint(toScreen: startPoint)
        return NSScreen.screens.first { $0.frame.contains(globalPoint) }
    }

    private func convertToCaptureRegion(_ rect: CGRect) -> CaptureRegion? {
        guard let window else { return nil }

        let globalRect = window.convertToScreen(rect)
        let targetScreen = activeScreen
            ?? NSScreen.screens.first {
                $0.frame.contains(CGPoint(x: globalRect.midX, y: globalRect.midY))
            }

        guard let targetScreen else { return nil }

        let clippedRect = globalRect.intersection(targetScreen.frame)
        guard !clippedRect.isNull, clippedRect.width > 0, clippedRect.height > 0 else {
            return nil
        }

        let localRect = CGRect(
            x: clippedRect.minX - targetScreen.frame.minX,
            y: targetScreen.frame.maxY - clippedRect.maxY,
            width: clippedRect.width,
            height: clippedRect.height
        )

        return CaptureRegion(
            rect: localRect,
            displayID: targetScreen.displayID,
            displayName: targetScreen.captureDisplayName
        )
    }
}

/// SwiftUI wrapper for presenting the region selector
struct RegionSelectorPresenter: NSViewRepresentable {
    let onRegionSelected: (CaptureRegion) -> Void
    let onCancelled: () -> Void

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            showSelector()
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}

    private func showSelector() {
        let window = RegionSelectorWindow()
        window.onRegionSelected = onRegionSelected
        window.onCancelled = onCancelled
        window.show()
    }
}

private extension NSScreen {
    var displayID: CGDirectDisplayID {
        deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID ?? 0
    }

    var captureDisplayName: String {
        localizedName
    }
}
