//
//  DocumentImportSection.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-03.
//

import SwiftUI
import UniformTypeIdentifiers

struct DocumentImportSection: View {
    @Environment(AppState.self) var appState
    @Bindable var importService: DocumentImportService
    let aiService: AIService

    private var canImport: Bool {
        appState.canUseImport && aiService.configurationIssue == nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Document Import")
                .font(.headline)

            Text("Convert PDFs and Word documents into markdown using the shared AI configuration above.")
                .foregroundStyle(.secondary)

            if let configurationIssue = aiService.configurationIssue {
                WorkflowNotice(
                    title: "AI Setup Required",
                    message: configurationIssue,
                    systemImage: "bolt.horizontal.circle"
                )
            }

            HStack {
                Spacer()

                if case .idle = importService.state {
                    Button("Import File...") {
                        selectFile()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!canImport)
                }
            }

            DocumentImportStatusView(importService: importService, aiService: aiService)
        }
        .padding()
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.35))
        .cornerRadius(16)
    }

    private func selectFile() {
        guard canImport else { return }

        let panel = NSOpenPanel()
        panel.allowedContentTypes = [
            .pdf,
            UTType(filenameExtension: "docx") ?? .data,
            UTType(filenameExtension: "doc") ?? .data
        ]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Select a PDF or Word document to convert to Markdown"
        panel.prompt = "Import"

        if panel.runModal() == .OK, let url = panel.url {
            do {
                let job = try DocumentImportJob(fileURL: url)
                importService.run(job: job, aiService: aiService)
            } catch {
                // Show error - the service will handle it
                print("Error creating import job: \(error)")
            }
        }
    }
}

struct DocumentImportStatusView: View {
    @Bindable var importService: DocumentImportService
    let aiService: AIService

    var body: some View {
        VStack(spacing: 12) {
            switch importService.state {
            case .idle:
                Label("Choose a PDF or Word document to convert into markdown", systemImage: "doc.badge.plus")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 20)

            case .converting:
                HStack {
                    ProgressView()
                        .scaleEffect(0.8)
                    Text("Converting document to images...")
                        .foregroundStyle(.blue)
                }

            case .processing(let current, let total):
                VStack(spacing: 8) {
                    ProgressView(value: Double(current), total: Double(total))
                    Text("AI Processing: \(current) / \(total) pages")

                    // Page thumbnails with status
                    if !importService.pageImages.isEmpty {
                        PageThumbnailsView(
                            images: importService.pageImages,
                            results: importService.processingResults
                        )
                    }
                }

                Button("Cancel") {
                    importService.cancel()
                }
                .buttonStyle(.bordered)
                .tint(.red)

            case .saving:
                HStack {
                    ProgressView()
                        .scaleEffect(0.8)
                    Text("Saving...")
                }

            case .completed(let url):
                VStack(spacing: 12) {
                    Label("Import Complete!", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                        .font(.headline)

                    Text(url.deletingLastPathComponent().path)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    HStack {
                        Button("Open Folder") {
                            NSWorkspace.shared.selectFile(url.path, inFileViewerRootedAtPath: url.deletingLastPathComponent().path)
                        }
                        Button("Open File") {
                            NSWorkspace.shared.open(url)
                        }
                        Button("Import Another") {
                            importService.reset()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }

            case .error(let message):
                VStack(spacing: 8) {
                    Label("Error", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Button("Try Again") {
                        importService.reset()
                    }
                    .buttonStyle(.bordered)
                }

            case .partiallyCompleted(let successCount, let failedCount):
                VStack(spacing: 12) {
                    Label("Partial Completion", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .font(.headline)

                    Text("\(successCount) pages succeeded, \(failedCount) failed")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)

                    // Page thumbnails
                    if !importService.pageImages.isEmpty {
                        PageThumbnailsView(
                            images: importService.pageImages,
                            results: importService.processingResults
                        )
                    }

                    HStack(spacing: 12) {
                        Button("Retry Failed") {
                            importService.retryFailed(aiService: aiService)
                        }
                        .buttonStyle(.borderedProminent)

                        Button("Save Anyway") {
                            importService.saveResults()
                        }
                        .buttonStyle(.bordered)
                    }
                }
            }
        }
    }
}

struct PageThumbnailsView: View {
    let images: [NSImage]
    let results: [ImageProcessingResult]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(images.enumerated()), id: \.offset) { index, image in
                    ZStack(alignment: .topTrailing) {
                        Image(nsImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(height: 60)
                            .cornerRadius(4)
                            .overlay(
                                RoundedRectangle(cornerRadius: 4)
                                    .stroke(statusColor(for: index), lineWidth: 2)
                            )

                        // Status indicator
                        if let result = results.first(where: { $0.id == index }) {
                            statusIcon(for: result.status)
                                .offset(x: 4, y: -4)
                        }
                    }
                }
            }
            .padding(.horizontal)
        }
        .frame(height: 70)
    }

    private func statusColor(for index: Int) -> Color {
        guard let result = results.first(where: { $0.id == index }) else {
            return Color.secondary.opacity(0.3)
        }
        switch result.status {
        case .pending: return Color.secondary.opacity(0.3)
        case .processing: return Color.blue
        case .success: return Color.green
        case .failed: return Color.red
        }
    }

    @ViewBuilder
    private func statusIcon(for status: ImageProcessingResult.Status) -> some View {
        switch status {
        case .pending:
            EmptyView()
        case .processing:
            ProgressView()
                .scaleEffect(0.5)
                .frame(width: 16, height: 16)
        case .success:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .font(.caption)
                .background(Circle().fill(.white).padding(2))
        case .failed:
            Image(systemName: "xmark.circle.fill")
                .foregroundStyle(.red)
                .font(.caption)
                .background(Circle().fill(.white).padding(2))
        }
    }
}

#Preview {
    DocumentImportSection(
        importService: DocumentImportService(),
        aiService: AIService.shared
    )
    .frame(width: 500)
    .padding()
}
