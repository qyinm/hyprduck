//
//  OpenRouterModels.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation

/// OpenRouter supported vision models
enum OpenRouterModels {
    static let all: [String] = [
        // OpenAI
        "openai/gpt-4.1-nano",
        "openai/gpt-4o",
        "openai/gpt-4o-mini",
        "openai/gpt-4-turbo",

        // Anthropic
        "anthropic/claude-3.5-sonnet",
        "anthropic/claude-3-opus",
        "anthropic/claude-3-haiku",

        // Google
        "google/gemini-2.0-flash-exp:free",
        "google/gemini-pro-vision",

        // Meta
        "meta-llama/llama-3.2-90b-vision-instruct",
        "meta-llama/llama-3.2-11b-vision-instruct",

        // Qwen
        "qwen/qwen-2-vl-72b-instruct",
        "qwen/qwen-2-vl-7b-instruct",
    ]

    static let defaultModel = "openai/gpt-4.1-nano"
}
