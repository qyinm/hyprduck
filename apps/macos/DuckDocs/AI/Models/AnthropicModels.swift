//
//  AnthropicModels.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation

/// Anthropic (Claude) supported vision models
enum AnthropicModels {
    static let all: [String] = [
        "claude-sonnet-4-20250514",
        "claude-3-5-sonnet-20241022",
        "claude-3-opus-20240229",
        "claude-3-sonnet-20240229",
        "claude-3-haiku-20240307",
    ]

    static let defaultModel = "claude-sonnet-4-20250514"
}
