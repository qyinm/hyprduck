//
//  WindowPickerView.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import SwiftUI
import ScreenCaptureKit

/// A view for selecting a window to capture
struct WindowPickerView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var windows: [SCWindow] = []
    @State private var isLoading = true
    @State private var errorMessage: String?
    @State private var selectedWindow: SCWindow?
    @State private var thumbnails: [CGWindowID: NSImage] = [:]

    let screenCapture = ScreenCapture()
    let onWindowSelected: (CGWindowID, String, String) -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Select Window")
                    .font(.headline)
                Spacer()
                Button("Cancel") {
                    dismiss()
                }
            }
            .padding()

            Divider()

            // Content
            if isLoading {
                ProgressView("Loading windows...")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = errorMessage {
                ContentUnavailableView(
                    "Error",
                    systemImage: "exclamationmark.triangle",
                    description: Text(error)
                )
            } else if windows.isEmpty {
                ContentUnavailableView(
                    "No Windows",
                    systemImage: "macwindow",
                    description: Text("No windows available for capture")
                )
            } else {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 200, maximum: 250))], spacing: 16) {
                        ForEach(windows, id: \.windowID) { window in
                            WindowCard(
                                window: window,
                                thumbnail: thumbnails[window.windowID],
                                isSelected: selectedWindow?.windowID == window.windowID
                            )
                            .onTapGesture {
                                selectedWindow = window
                            }
                        }
                    }
                    .padding()
                }
            }

            Divider()

            // Footer
            HStack {
                if let window = selectedWindow {
                    Text("Selected: \(window.displayName)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Button("Select") {
                    if let window = selectedWindow {
                        let appName = window.owningApplication?.applicationName ?? "Unknown App"
                        let title = window.title ?? ""
                        onWindowSelected(window.windowID, title, appName)
                        dismiss()
                    }
                }
                .disabled(selectedWindow == nil)
                .buttonStyle(.borderedProminent)
            }
            .padding()
        }
        .frame(width: 600, height: 500)
        .task {
            await loadWindows()
        }
    }

    private func loadWindows() async {
        isLoading = true
        errorMessage = nil

        do {
            let allWindows = try await screenCapture.getWindows()
            // Filter to only show windows with valid frames and from user applications
            windows = allWindows.filter { window in
                window.frame.width >= 100 &&
                window.frame.height >= 100 &&
                window.owningApplication != nil &&
                window.isOnScreen
            }

            // Load thumbnails for each window
            await loadThumbnails()

            isLoading = false
        } catch {
            errorMessage = error.localizedDescription
            isLoading = false
        }
    }

    private func loadThumbnails() async {
        for window in windows {
            do {
                let image = try await screenCapture.captureWindow(window)
                // Resize to thumbnail
                let thumbnailSize = NSSize(width: 180, height: 120)
                let thumbnail = resizeImage(image, to: thumbnailSize)
                thumbnails[window.windowID] = thumbnail
            } catch {
                // Skip windows that can't be captured
            }
        }
    }

    private func resizeImage(_ image: NSImage, to size: NSSize) -> NSImage {
        let newImage = NSImage(size: size)
        newImage.lockFocus()
        image.draw(
            in: NSRect(origin: .zero, size: size),
            from: NSRect(origin: .zero, size: image.size),
            operation: .copy,
            fraction: 1.0
        )
        newImage.unlockFocus()
        return newImage
    }
}

/// Card view for displaying a window option
struct WindowCard: View {
    let window: SCWindow
    let thumbnail: NSImage?
    let isSelected: Bool

    var body: some View {
        VStack(spacing: 8) {
            // Thumbnail
            ZStack {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(nsColor: .windowBackgroundColor))

                if let thumbnail {
                    Image(nsImage: thumbnail)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .padding(4)
                } else {
                    Image(systemName: "macwindow")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(height: 120)
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(isSelected ? Color.accentColor : Color.clear, lineWidth: 3)
            )

            // Window info
            VStack(spacing: 2) {
                if let appName = window.owningApplication?.applicationName {
                    Text(appName)
                        .font(.caption)
                        .fontWeight(.medium)
                        .lineLimit(1)
                }

                if let title = window.title, !title.isEmpty {
                    Text(title)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(8)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(isSelected ? Color.accentColor.opacity(0.1) : Color.clear)
        )
    }
}

#Preview {
    WindowPickerView { windowID, title, appName in
        print("Selected: \(appName) - \(title) (\(windowID))")
    }
}
