//
//  AppState.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation
import SwiftUI

/// Global application state for shared user-facing errors.
@Observable
@MainActor
final class AppState {
    static let shared = AppState()

    var errorMessage: String?
    var showError: Bool = false

    private init() {}

    func showError(message: String) {
        errorMessage = message
        showError = true
    }

    func clearError() {
        errorMessage = nil
        showError = false
    }
}
