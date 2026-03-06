//
//  MarkdownGenerator.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import AppKit

/// Generates markdown documentation for capture and import workflows.
struct MarkdownGenerator {
    struct Configuration {
        var includeTableOfContents: Bool = true
        var includeTimestamps: Bool = false
    }

    struct Section {
        let title: String
        let detail: String?
        let imagePath: String
        let body: String
    }

    var configuration = Configuration()

    func generate(title: String, sections: [Section]) -> String {
        var markdown = "# \(title)\n\n"

        if configuration.includeTimestamps {
            let formatter = DateFormatter()
            formatter.dateStyle = .long
            formatter.timeStyle = .short
            markdown += "*Generated: \(formatter.string(from: Date()))*\n\n"
        }

        if configuration.includeTableOfContents && sections.count > 1 {
            markdown += "## Table of Contents\n\n"
            for section in sections {
                markdown += "- [\(section.title)](#\(anchor(for: section.title)))\n"
            }
            markdown += "\n---\n\n"
        }

        for (index, section) in sections.enumerated() {
            markdown += "## \(section.title)\n\n"

            if let detail = section.detail, !detail.isEmpty {
                markdown += "\(detail)\n\n"
            }

            markdown += "![\(section.title)](\(section.imagePath))\n\n"
            markdown += section.body.trimmingCharacters(in: .whitespacesAndNewlines) + "\n"

            if index < sections.count - 1 {
                markdown += "\n\n---\n\n"
            } else {
                markdown += "\n"
            }
        }

        return markdown
    }

    func generate(
        title: String,
        captures: [CaptureResult],
        aiAnalysis: [String]? = nil
    ) -> String {
        let sections = captures.enumerated().map { index, capture in
            Section(
                title: "Step \(capture.stepNumber)",
                detail: "**Action:** \(capture.action.description)",
                imagePath: "images/step_\(capture.stepNumber).png",
                body: aiAnalysis?[safe: index] ?? ""
            )
        }

        return generate(title: title, sections: sections)
    }

    func export(
        title: String,
        captures: [CaptureResult],
        aiAnalysis: [String]? = nil,
        to directory: URL
    ) throws -> URL {
        let imagesDir = directory.appendingPathComponent("images", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: imagesDir, withIntermediateDirectories: true)

        for capture in captures {
            let imageName = "step_\(capture.stepNumber).png"
            let imageURL = imagesDir.appendingPathComponent(imageName)

            guard let tiffData = capture.screenshot.tiffRepresentation,
                  let bitmap = NSBitmapImageRep(data: tiffData),
                  let imageData = bitmap.representation(using: .png, properties: [:]) else {
                continue
            }

            try imageData.write(to: imageURL)
        }

        let markdown = generate(title: title, captures: captures, aiAnalysis: aiAnalysis)
        let markdownURL = directory.appendingPathComponent("README.md")
        try markdown.write(to: markdownURL, atomically: true, encoding: .utf8)

        return markdownURL
    }

    private func anchor(for title: String) -> String {
        title
            .lowercased()
            .replacingOccurrences(of: "[^a-z0-9\\s-]", with: "", options: .regularExpression)
            .replacingOccurrences(of: "\\s+", with: "-", options: .regularExpression)
    }
}

extension Action {
    /// Short description for legacy capture table-of-contents entries.
    var shortDescription: String {
        switch self {
        case .click(_, _, let button):
            return "\(button.rawValue.capitalized) Click"
        case .doubleClick(_, _, let button):
            return "\(button.rawValue.capitalized) Double-Click"
        case .drag:
            return "Drag"
        case .scroll:
            return "Scroll"
        case .keyPress(_, let character, let modifiers):
            let char = character ?? "Key"
            if !modifiers.isEmpty {
                return "\(modifiers.description)\(char)"
            }
            return "Key '\(char)'"
        case .typeText(let text):
            let preview = text.count > 10 ? String(text.prefix(10)) + "..." : text
            return "Type \"\(preview)\""
        case .delay(let seconds):
            return "Wait \(String(format: "%.1f", seconds))s"
        }
    }
}

extension Array {
    subscript(safe index: Index) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
