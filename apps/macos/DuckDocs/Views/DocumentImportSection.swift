//
//  DocumentImportSection.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-03.
//

import SwiftUI
import UniformTypeIdentifiers

enum ImportPanelPresenter {
    @MainActor
    static func present(importService: DocumentImportService, aiService: AIService) {
        guard aiService.configurationIssue == nil, RustParsingEngine.isAvailable else { return }

        let panel = NSOpenPanel()
        panel.allowedContentTypes = [
            .pdf,
            UTType(filenameExtension: "docx") ?? .data,
            UTType(filenameExtension: "doc") ?? .data
        ]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Select a PDF or Word document to parse into Markdown"
        panel.prompt = "Parse"

        if panel.runModal() == .OK, let url = panel.url {
            do {
                let job = try DocumentImportJob(fileURL: url)
                importService.run(job: job, aiService: aiService)
            } catch {
                print("Error creating import job: \(error)")
            }
        }
    }
}

struct DocumentImportSection: View {
    @Bindable var importService: DocumentImportService
    let aiService: AIService

    private var canImport: Bool {
        importIssue == nil
    }

    private var importIssue: String? {
        if let configurationIssue = aiService.configurationIssue {
            return configurationIssue
        }

        guard RustParsingEngine.isAvailable else {
            return "Rust parsing engine is unavailable. Build or bundle `duckdocs-engine` to import documents."
        }

        return nil
    }

    var body: some View {
        WorkflowCard(
            eyebrow: "File Parsing",
            title: "Bring in PDFs and Word files",
            description: "Start from an existing file, extract each page as an image, and generate markdown with the shared AI engine."
        ) {
            VStack(alignment: .leading, spacing: 16) {
                SupportedFormatsRow()
                DocumentImportStatusView(
                    importService: importService,
                    aiService: aiService,
                    canImport: canImport,
                    importIssue: importIssue
                )
            }
        }
    }
}

struct SupportedFormatsRow: View {
    private let formats = [
        ("PDF", "doc.richtext"),
        ("DOCX", "text.document"),
        ("DOC", "doc")
    ]

    var body: some View {
        HStack(spacing: 8) {
            ForEach(formats, id: \.0) { format in
                Label(format.0, systemImage: format.1)
                    .font(.caption)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                    .background(Color.white.opacity(0.55), in: Capsule())
            }
        }
    }
}

struct DocumentImportStatusView: View {
    @Bindable var importService: DocumentImportService
    let aiService: AIService
    let canImport: Bool
    let importIssue: String?

    var body: some View {
        VStack(spacing: 12) {
            switch importService.state {
            case .idle:
                IdleImportPanel(canImport: canImport, disabledReason: importIssue) {
                    ImportPanelPresenter.present(importService: importService, aiService: aiService)
                }

            case .converting:
                WorkflowProgressPanel(
                    title: "Preparing document pages",
                    subtitle: "Converting the selected file into page images.",
                    tint: .blue
                ) {
                    ProgressView()
                }

            case .processing(let current, let total):
                WorkflowProgressPanel(
                    title: "Analyzing imported pages",
                    subtitle: "AI is converting page \(current) of \(total) into markdown.",
                    tint: .blue
                ) {
                    VStack(spacing: 12) {
                        ProgressView(value: Double(current), total: Double(total))
                        if !importService.pageImages.isEmpty {
                            PageThumbnailsView(
                                images: importService.pageImages,
                                results: importService.processingResults
                            )
                        }

                        Button("Cancel") {
                            importService.cancel()
                        }
                        .buttonStyle(.bordered)
                        .tint(.red)
                    }
                }

            case .saving:
                WorkflowProgressPanel(
                    title: "Saving markdown package",
                    subtitle: "Writing the markdown file and linked page images.",
                    tint: .orange
                ) {
                    ProgressView()
                }

            case .completed(let url):
                WorkflowProgressPanel(
                    title: "Import complete",
                    subtitle: url.deletingLastPathComponent().path,
                    tint: .green
                ) {
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
                WorkflowProgressPanel(
                    title: "Import failed",
                    subtitle: message,
                    tint: .red
                ) {
                    Button("Try Again") {
                        importService.reset()
                    }
                    .buttonStyle(.bordered)
                }

            case .partiallyCompleted(let successCount, let failedCount):
                WorkflowProgressPanel(
                    title: "Partial import",
                    subtitle: "\(successCount) pages succeeded, \(failedCount) failed.",
                    tint: .orange
                ) {
                    VStack(spacing: 12) {
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
}

struct IdleImportPanel: View {
    let canImport: Bool
    let disabledReason: String?
    let importAction: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "square.and.arrow.down.on.square")
                .font(.system(size: 32))
                .foregroundStyle(Color.accentColor)

            VStack(spacing: 6) {
                Text("Import a document to begin")
                    .font(.headline)
                Text("Pick a PDF or Word document and DuckDocs will turn each page into a linked markdown package.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            Button(action: importAction) {
                Label("Choose File", systemImage: "doc.badge.plus")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 10)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!canImport)

            if let disabledReason, !canImport {
                Text(disabledReason)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(24)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(Color.white.opacity(0.58))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(style: StrokeStyle(lineWidth: 1.5, dash: [8, 8]))
                .foregroundStyle(Color.accentColor.opacity(0.35))
        )
    }
}

struct WorkflowProgressPanel<Content: View>: View {
    let title: String
    let subtitle: String
    let tint: Color
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Circle()
                    .fill(tint.opacity(0.18))
                    .frame(width: 36, height: 36)
                    .overlay(
                        Image(systemName: "sparkles.rectangle.stack")
                            .foregroundStyle(tint)
                    )

                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.headline)
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                }
            }

            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(18)
        .background(Color.white.opacity(0.58), in: RoundedRectangle(cornerRadius: 18))
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
                            .frame(height: 68)
                            .cornerRadius(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 8)
                                    .stroke(statusColor(for: index), lineWidth: 2)
                            )

                        if let result = results.first(where: { $0.id == index }) {
                            statusIcon(for: result.status)
                                .offset(x: 4, y: -4)
                        }
                    }
                }
            }
            .padding(.horizontal, 2)
        }
        .frame(height: 80)
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
