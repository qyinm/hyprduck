//
//  OnboardingView.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import SwiftUI

/// Onboarding view for permission setup
struct OnboardingView: View {
    @Environment(AppState.self) var appState
    @Environment(\.dismiss) private var dismiss
    @AppStorage("hasDismissedCapturePermissionOnboarding") private var hasDismissedCapturePermissionOnboarding = false

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

                Text("Capture screens or import documents, then generate markdown with AI.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
            }

            Divider()

            // Permissions
            VStack(alignment: .leading, spacing: 24) {
                Text("Capture Permissions")
                    .font(.headline)

                PermissionRow(
                    title: "Accessibility",
                    description: "Required to automate clicks and keyboard actions during capture",
                    systemImage: "hand.tap",
                    isGranted: appState.permissionManager.accessibilityGranted,
                    action: {
                        appState.permissionManager.requestAccessibilityPermission()
                    }
                )

                PermissionRow(
                    title: "Screen Recording",
                    description: "Required to capture screenshots during Capture Workflow",
                    systemImage: "rectangle.dashed.badge.record",
                    isGranted: appState.permissionManager.screenCaptureGranted,
                    action: {
                        appState.permissionManager.requestScreenCapturePermission()
                    }
                )

                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "doc.text")
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Document Import is available now")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                        Text("You can import PDFs and Word documents without granting capture permissions.")
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
                hasDismissedCapturePermissionOnboarding = true
                dismiss()
            } label: {
                Text(appState.permissionManager.canUseCapture ? "Get Started" : "Continue to App")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)

            if !appState.permissionManager.canUseCapture {
                Text("Capture stays disabled until you grant both permissions. Document Import already works.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(40)
        .frame(width: 500, height: 600)
        .task {
            await appState.permissionManager.checkAllPermissions()
        }
    }
}

// MARK: - Permission Row

struct PermissionRow: View {
    let title: String
    let description: String
    let systemImage: String
    let isGranted: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 16) {
            Image(systemName: systemImage)
                .font(.title2)
                .frame(width: 32)
                .foregroundStyle(isGranted ? .green : .secondary)

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)

                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if isGranted {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .font(.title2)
            } else {
                Button("Grant") {
                    action()
                }
                .buttonStyle(.bordered)
            }
        }
    }
}

#Preview {
    OnboardingView()
        .environment(AppState.shared)
}
