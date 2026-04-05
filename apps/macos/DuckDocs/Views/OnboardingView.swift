//
//  OnboardingView.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import SwiftUI

/// Onboarding view for permission setup
struct OnboardingView: View {
    @Environment(\.dismiss) private var dismiss
    @AppStorage("hasDismissedFileParsingOnboarding") private var hasDismissedFileParsingOnboarding = false

    var body: some View {
        VStack(spacing: 32) {
            // Header
            VStack(spacing: 12) {
                Image(systemName: "doc.text.magnifyingglass")
                    .font(.system(size: 64))
                    .foregroundStyle(.tint)

                Text("Welcome to DuckDocs")
                    .font(.largeTitle)
                    .fontWeight(.bold)

                Text("Parse documents into markdown with AI.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
            }

            Divider()

            VStack(alignment: .leading, spacing: 24) {
                Text("Getting Started")
                    .font(.headline)

                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "doc.text")
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("File parsing is available immediately")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                        Text("You can import PDFs and Word documents without granting screen or accessibility permissions.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "bolt.horizontal.circle")
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Configure AI when you are ready")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                        Text("Pick a provider, model, and prompt template from the advanced settings panel before parsing larger files.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding()
            .background(.regularMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            Spacer()

            // Continue button
            Button {
                hasDismissedFileParsingOnboarding = true
                dismiss()
            } label: {
                Text("Continue to App")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
        .padding(40)
        .frame(width: 500, height: 520)
    }
}

#Preview {
    OnboardingView()
        .environment(AppState.shared)
}
