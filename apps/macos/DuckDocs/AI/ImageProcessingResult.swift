//
//  ImageProcessingResult.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-01.
//

import Foundation
import AppKit

/// Tracks the processing status of each captured image
struct ImageProcessingResult: Identifiable {
    let id: Int // Index of the image
    let image: NSImage
    var status: Status
    var analysis: String?
    var errorMessage: String?

    enum Status {
        case pending
        case processing
        case success
        case failed
    }
}
