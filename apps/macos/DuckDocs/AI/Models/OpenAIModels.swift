//
//  OpenAIModels.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation

/// OpenAI direct API supported vision models
enum OpenAIModels {
    static let all: [String] = [
        "gpt-4o",
        "gpt-4o-mini",
        "gpt-4-turbo",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4.1-nano",
    ]

    static let defaultModel = "gpt-4o"
}
