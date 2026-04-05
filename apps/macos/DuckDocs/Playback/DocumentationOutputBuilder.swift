//
//  DocumentationOutputBuilder.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-03-06.
//

import Foundation
import AppKit

/// Shared export pipeline for capture and import workflows.
struct DocumentationOutputBuilder {
    private let generator = MarkdownGenerator()

    func exportCapture(job: CaptureJob, results: [ImageProcessingResult]) throws -> URL {
        let orderedResults = results.sorted { $0.id < $1.id }

        let sections = orderedResults.enumerated().map { index, result in
            MarkdownGenerator.Section(
                title: "Step \(index + 1)",
                detail: captureDetail(for: index, job: job),
                imagePath: "images/step_\(index + 1).png",
                body: body(for: result, itemName: "step", number: index + 1)
            )
        }

        return try exportDocument(
            outputName: job.outputName,
            title: job.outputName,
            imagePrefix: "step",
            sections: sections,
            images: orderedResults.map(\.image)
        )
    }

    func exportImport(job: DocumentImportJob, results: [ImageProcessingResult]) throws -> URL {
        let orderedResults = results.sorted { $0.id < $1.id }
        let pages = orderedResults.enumerated().map { index, result in
            SchemaParsedPage(
                index: index,
                markdown: result.analysis,
                plainText: result.analysis,
                svg: nil
            )
        }
        let metadata = SchemaParseMetadata(
            engineID: "legacy-image-processing",
            durationMilliseconds: 0,
            pageCount: orderedResults.count
        )
        let parseResult = SchemaParseResult(
            markdown: orderedResults.enumerated().map { index, result in
                generator.generate(
                    title: job.outputName,
                    sections: [
                        MarkdownGenerator.Section(
                            title: "Page \(index + 1)",
                            detail: "**Source:** \(job.format.displayName)",
                            imagePath: "images/page_\(index + 1).png",
                            body: body(for: result, itemName: "page", number: index + 1)
                        )
                    ]
                )
            }.joined(separator: "\n\n"),
            pages: pages,
            assets: [],
            metadata: metadata
        )

        return try exportImport(job: job, parseResult: parseResult, images: orderedResults.map(\.image))
    }

    func exportImport(job: DocumentImportJob, parseResult: SchemaParseResult, images: [NSImage]) throws -> URL {
        let sections = parseResult.pages.enumerated().map { index, page in
            MarkdownGenerator.Section(
                title: "Page \(index + 1)",
                detail: "**Source:** \(job.format.displayName)",
                imagePath: "images/page_\(index + 1).png",
                body: pageBody(for: page, number: index + 1)
            )
        }

        return try exportDocument(
            outputName: job.outputName,
            title: job.outputName,
            imagePrefix: "page",
            sections: sections,
            images: images
        )
    }

    private func exportDocument(
        outputName: String,
        title: String,
        imagePrefix: String,
        sections: [MarkdownGenerator.Section],
        images: [NSImage]
    ) throws -> URL {
        let outputDir = makeOutputDirectory(outputName: outputName)
        let imagesDir = outputDir.appendingPathComponent("images", isDirectory: true)
        try FileManager.default.createDirectory(at: imagesDir, withIntermediateDirectories: true)

        for (index, image) in images.enumerated() {
            let filename = "\(imagePrefix)_\(index + 1).png"
            let imageURL = imagesDir.appendingPathComponent(filename)
            try writePNG(image: image, to: imageURL)
        }

        let markdown = generator.generate(title: title, sections: sections)
        let markdownURL = outputDir.appendingPathComponent("\(sanitize(outputName)).md")
        try markdown.write(to: markdownURL, atomically: true, encoding: .utf8)

        return markdownURL
    }

    private func makeOutputDirectory(outputName: String) -> URL {
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        let timestamp = ISO8601DateFormatter().string(from: Date()).replacingOccurrences(of: ":", with: "-")
        let safeName = sanitize(outputName)
        return documentsDir.appendingPathComponent("DuckDocs/\(safeName)_\(timestamp)", isDirectory: true)
    }

    private func sanitize(_ outputName: String) -> String {
        let sanitizedName = outputName
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: "\\", with: "-")
            .replacingOccurrences(of: ":", with: "-")
            .replacingOccurrences(of: "..", with: "-")
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .prefix(100)

        return sanitizedName.isEmpty ? "output" : String(sanitizedName)
    }

    private func writePNG(image: NSImage, to url: URL) throws {
        guard let tiffData = image.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiffData),
              let pngData = bitmap.representation(using: .png, properties: [:]) else {
            throw CaptureError.imageConversionFailed
        }

        try pngData.write(to: url)
    }

    private func body(for result: ImageProcessingResult, itemName: String, number: Int) -> String {
        if let analysis = result.analysis?.trimmingCharacters(in: .whitespacesAndNewlines), !analysis.isEmpty {
            return analysis
        }

        if let errorMessage = result.errorMessage, !errorMessage.isEmpty {
            return "_AI analysis unavailable for \(itemName) \(number): \(errorMessage)_"
        }

        return "_AI analysis unavailable for \(itemName) \(number)._"
    }

    private func pageBody(for page: SchemaParsedPage, number: Int) -> String {
        if let markdown = page.markdown?.trimmingCharacters(in: .whitespacesAndNewlines), !markdown.isEmpty {
            return markdown
        }

        if let plainText = page.plainText?.trimmingCharacters(in: .whitespacesAndNewlines), !plainText.isEmpty {
            return plainText
        }

        return "_AI analysis unavailable for page \(number)._"
    }

    private func captureDetail(for index: Int, job: CaptureJob) -> String {
        if index == 0 {
            return "**Capture:** Initial state"
        }

        switch job.nextAction {
        case .none:
            return "**Capture:** Manual progression before this step"
        default:
            return "**Capture:** After \(job.nextAction.displayName)"
        }
    }
}
