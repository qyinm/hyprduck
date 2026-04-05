//
//  PromptTemplate.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-02-01.
//

import Foundation

enum PromptTemplate: String, CaseIterable, Identifiable, Codable {
    case general = "General"
    case apiDocs = "API Documentation"
    case uiFlow = "UI Flow"
    case tutorial = "Tutorial"
    case codeSnippets = "Code Snippets"
    case dataTables = "Data Tables"

    var id: String { rawValue }

    var prompt: String {
        switch self {
        case .general:
            return "Convert this image to well-structured markdown. Extract all text and preserve the layout."
        case .apiDocs:
            return "Convert this image to markdown formatted as API documentation. Extract endpoint paths, methods, parameters, request/response bodies, and status codes. Use proper code blocks for examples."
        case .uiFlow:
            return "Convert this image to markdown describing a UI flow. Identify UI elements, buttons, inputs, and describe the user interaction flow step by step."
        case .tutorial:
            return "Convert this image to a tutorial-style markdown document. Create numbered steps, highlight important actions, and add helpful notes for the reader."
        case .codeSnippets:
            return "Extract code from this image into properly formatted markdown code blocks. Identify the programming language and use appropriate syntax highlighting tags."
        case .dataTables:
            return "Convert this image to markdown, focusing on extracting tabular data into proper markdown tables. Preserve headers and data alignment."
        }
    }

    var icon: String {
        switch self {
        case .general: return "doc.text"
        case .apiDocs: return "chevron.left.forwardslash.chevron.right"
        case .uiFlow: return "rectangle.connected.to.line.below"
        case .tutorial: return "list.number"
        case .codeSnippets: return "curlybraces"
        case .dataTables: return "tablecells"
        }
    }
}
