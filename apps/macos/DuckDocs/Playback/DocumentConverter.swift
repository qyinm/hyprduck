//
//  DocumentConverter.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-03.
//

import Foundation
import AppKit
import PDFKit

/// Converts documents to images for AI processing
actor DocumentConverter {

    /// Convert a document to an array of images (one per page)
    func convert(_ job: DocumentImportJob) async throws -> [NSImage] {
        switch job.format {
        case .pdf:
            return try await convertPDF(job.fileURL)
        case .docx, .doc:
            return try await convertWord(job.fileURL)
        }
    }

    // MARK: - PDF Conversion

    private func convertPDF(_ url: URL) async throws -> [NSImage] {
        guard let document = PDFDocument(url: url) else {
            throw DocumentImportError.conversionFailed("Could not open PDF")
        }

        guard document.pageCount > 0 else {
            throw DocumentImportError.emptyDocument
        }

        var images: [NSImage] = []

        for i in 0..<document.pageCount {
            guard let page = document.page(at: i) else { continue }

            if let image = renderPDFPage(page, scale: 2.0) {
                images.append(image)
            }
        }

        guard !images.isEmpty else {
            throw DocumentImportError.conversionFailed("Could not render any pages")
        }

        return images
    }

    private func renderPDFPage(_ page: PDFPage, scale: CGFloat) -> NSImage? {
        let bounds = page.bounds(for: .mediaBox)
        let size = NSSize(width: bounds.width * scale, height: bounds.height * scale)

        let image = NSImage(size: size)
        image.lockFocus()

        if let context = NSGraphicsContext.current?.cgContext {
            // White background
            context.setFillColor(NSColor.white.cgColor)
            context.fill(CGRect(origin: .zero, size: size))

            // Scale and draw
            context.scaleBy(x: scale, y: scale)
            page.draw(with: .mediaBox, to: context)
        }

        image.unlockFocus()
        return image
    }

    // MARK: - Word Document Conversion

    private func convertWord(_ url: URL) async throws -> [NSImage] {
        // Load document as attributed string
        guard let attributedString = try? NSAttributedString(
            url: url,
            options: [.documentType: NSAttributedString.DocumentType.docFormat],
            documentAttributes: nil
        ) else {
            // Try as DOCX (Office Open XML)
            guard let attributedString = try? NSAttributedString(
                url: url,
                options: [:],
                documentAttributes: nil
            ) else {
                throw DocumentImportError.conversionFailed("Could not read Word document")
            }
            return renderAttributedString(attributedString)
        }

        return renderAttributedString(attributedString)
    }

    private func renderAttributedString(_ attributedString: NSAttributedString) -> [NSImage] {
        // Create a text container for pagination
        let pageWidth: CGFloat = 612 // US Letter width in points
        let pageHeight: CGFloat = 792 // US Letter height in points
        let margin: CGFloat = 72 // 1 inch margins

        let textWidth = pageWidth - (margin * 2)
        let textHeight = pageHeight - (margin * 2)

        let textStorage = NSTextStorage(attributedString: attributedString)
        let layoutManager = NSLayoutManager()
        textStorage.addLayoutManager(layoutManager)

        var images: [NSImage] = []
        var currentLocation = 0
        let totalLength = attributedString.length

        while currentLocation < totalLength {
            let textContainer = NSTextContainer(size: NSSize(width: textWidth, height: textHeight))
            textContainer.lineFragmentPadding = 0
            layoutManager.addTextContainer(textContainer)

            // Force layout
            layoutManager.ensureLayout(for: textContainer)

            // Get the range for this container
            let glyphRange = layoutManager.glyphRange(for: textContainer)
            let characterRange = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)

            if characterRange.length == 0 {
                break
            }

            // Render page
            let image = NSImage(size: NSSize(width: pageWidth, height: pageHeight))
            image.lockFocus()

            // White background
            NSColor.white.setFill()
            NSRect(origin: .zero, size: NSSize(width: pageWidth, height: pageHeight)).fill()

            // Draw text
            let origin = NSPoint(x: margin, y: margin)
            layoutManager.drawGlyphs(forGlyphRange: glyphRange, at: origin)

            image.unlockFocus()
            images.append(image)

            currentLocation = characterRange.location + characterRange.length
        }

        // If no images were created, create at least one with the full content
        if images.isEmpty && attributedString.length > 0 {
            let size = NSSize(width: pageWidth, height: pageHeight)
            let image = NSImage(size: size)
            image.lockFocus()

            NSColor.white.setFill()
            NSRect(origin: .zero, size: size).fill()

            attributedString.draw(in: NSRect(x: margin, y: margin, width: textWidth, height: textHeight))

            image.unlockFocus()
            images.append(image)
        }

        return images
    }
}
