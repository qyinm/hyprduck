//
//  CapturePreviewWindow.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-01.
//

import AppKit
import SwiftUI

/// A floating preview window that shows what will be captured during countdown
class CapturePreviewWindow: NSPanel {
    private var countdownLabel: NSTextField!
    private var previewImageView: NSImageView!
    private var infoLabel: NSTextField!
    private var cancelButton: NSButton!

    var onCancel: (() -> Void)?

    private var countdown: Int = 3
    private var countdownTimer: Timer?

    init(previewImage: NSImage, captureMode: CaptureMode) {
        let windowSize = CGSize(width: 320, height: 280)
        let screenFrame = NSScreen.main?.visibleFrame ?? .zero
        let windowFrame = CGRect(
            x: screenFrame.maxX - windowSize.width - 20,
            y: screenFrame.maxY - windowSize.height - 20,
            width: windowSize.width,
            height: windowSize.height
        )

        super.init(
            contentRect: windowFrame,
            styleMask: [.titled, .nonactivatingPanel, .hudWindow],
            backing: .buffered,
            defer: false
        )

        title = "Capture Preview"
        level = .floating
        isFloatingPanel = true
        becomesKeyOnlyIfNeeded = true
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        setupUI(previewImage: previewImage, captureMode: captureMode)
    }

    private func setupUI(previewImage: NSImage, captureMode: CaptureMode) {
        let containerView = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 280))

        // Preview image
        previewImageView = NSImageView(frame: NSRect(x: 10, y: 80, width: 300, height: 170))
        previewImageView.image = previewImage
        previewImageView.imageScaling = .scaleProportionallyUpOrDown
        previewImageView.wantsLayer = true
        previewImageView.layer?.cornerRadius = 8
        previewImageView.layer?.masksToBounds = true
        previewImageView.layer?.borderColor = NSColor.separatorColor.cgColor
        previewImageView.layer?.borderWidth = 1
        containerView.addSubview(previewImageView)

        // Info label (capture mode)
        infoLabel = NSTextField(labelWithString: "Target: \(captureMode.displayName)")
        infoLabel.frame = NSRect(x: 10, y: 55, width: 300, height: 20)
        infoLabel.font = NSFont.systemFont(ofSize: 12)
        infoLabel.textColor = NSColor.secondaryLabelColor
        infoLabel.alignment = .center
        containerView.addSubview(infoLabel)

        // Countdown label
        countdownLabel = NSTextField(labelWithString: "Starting in 3...")
        countdownLabel.frame = NSRect(x: 10, y: 250, width: 300, height: 24)
        countdownLabel.font = NSFont.systemFont(ofSize: 16, weight: .semibold)
        countdownLabel.textColor = NSColor.labelColor
        countdownLabel.alignment = .center
        containerView.addSubview(countdownLabel)

        // Cancel button
        cancelButton = NSButton(title: "Cancel (ESC)", target: self, action: #selector(cancelClicked))
        cancelButton.frame = NSRect(x: 100, y: 15, width: 120, height: 32)
        cancelButton.bezelStyle = .rounded
        cancelButton.keyEquivalent = "\u{1b}" // ESC key
        containerView.addSubview(cancelButton)

        contentView = containerView
    }

    @objc private func cancelClicked() {
        stopCountdown()
        onCancel?()
        close()
    }

    func startCountdown(completion: @escaping () -> Void) {
        countdown = 3
        updateCountdownLabel()

        countdownTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] timer in
            guard let self = self else {
                timer.invalidate()
                return
            }

            self.countdown -= 1

            if self.countdown <= 0 {
                timer.invalidate()
                self.countdownTimer = nil
                self.close()
                completion()
            } else {
                self.updateCountdownLabel()
            }
        }
    }

    func stopCountdown() {
        countdownTimer?.invalidate()
        countdownTimer = nil
    }

    private func updateCountdownLabel() {
        countdownLabel.stringValue = "Starting in \(countdown)..."
    }

    func show() {
        makeKeyAndOrderFront(nil)
    }

    deinit {
        stopCountdown()
    }
}
