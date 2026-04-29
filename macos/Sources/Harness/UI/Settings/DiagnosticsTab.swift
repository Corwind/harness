import SwiftUI
import AppKit

/// Settings → Diagnostics tab. Polls the backend's log tail, surfaces a
/// retryable banner when the backend is offline, and offers Copy /
/// Export-support-bundle actions.
struct DiagnosticsTab: View {
    @Environment(\.theme) private var theme
    @Bindable var viewModel: DiagnosticsViewModel
    /// Reference to the SettingsViewModel so the support bundle can
    /// snapshot the current providers list (with API keys redacted).
    let settings: SettingsViewModel

    private static let bottomAnchor = "diagnostics.bottom.anchor"

    var body: some View {
        VStack(spacing: 0) {
            if let error = viewModel.lastError {
                bannerView(error)
            }
            logScroll
            Divider()
            actionBar
        }
        .background(theme.background)
        .foregroundStyle(theme.text)
        .task {
            await viewModel.start()
        }
        .onDisappear {
            Task { await viewModel.stop() }
        }
    }

    private func bannerView(_ error: BackendError) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(theme.error)
            Text(bannerMessage(for: error))
                .font(.callout)
                .foregroundStyle(theme.text)
                .lineLimit(2)
            Spacer()
            Button("Retry now") {
                Task { await viewModel.pollOnce() }
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(theme.error.opacity(0.08))
    }

    private func bannerMessage(for error: BackendError) -> String {
        switch error {
        case .transport: return "Backend offline. Retrying…"
        case .httpStatus(let status, _): return "Backend error (\(status)). Retrying…"
        case .decoding, .encoding, .malformedResponse, .malformedEvent:
            return "Backend response was malformed. Retrying…"
        case .cancelled: return "Polling cancelled."
        }
    }

    private var logScroll: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if viewModel.lines.isEmpty && viewModel.lastError == nil {
                        Text("Waiting for log lines…")
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(theme.mutedText)
                            .padding(8)
                    } else {
                        ForEach(viewModel.lines) { line in
                            row(for: line)
                                .id(line.seq)
                        }
                    }
                    Color.clear.frame(height: 1).id(Self.bottomAnchor)
                }
                .padding(8)
            }
            .background(theme.codeBackground)
            .onChange(of: viewModel.lines.count) { _, _ in
                withAnimation(.easeOut(duration: 0.1)) {
                    proxy.scrollTo(Self.bottomAnchor, anchor: .bottom)
                }
            }
        }
    }

    private func row(for line: LogLine) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text("[\(String(line.seq))]")
                .foregroundStyle(theme.mutedText)
            Text(line.ts)
                .foregroundStyle(theme.mutedText)
            Text(line.level.rawValue.uppercased())
                .foregroundStyle(color(for: line.level))
            Text(line.target)
                .foregroundStyle(theme.mutedText)
            Text(line.message)
                .foregroundStyle(theme.codeText)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
        .font(.system(.caption, design: .monospaced))
    }

    private func color(for level: LogLevel) -> Color {
        switch level {
        case .error: return theme.error
        case .warn: return theme.accent
        case .info: return theme.text
        case .debug, .trace: return theme.mutedText
        }
    }

    private var actionBar: some View {
        HStack(spacing: 8) {
            Button {
                copyLogsToPasteboard()
            } label: {
                Label("Copy", systemImage: "doc.on.doc")
            }
            .disabled(viewModel.lines.isEmpty)

            Button {
                viewModel.clear()
            } label: {
                Label("Clear", systemImage: "trash")
            }
            .disabled(viewModel.lines.isEmpty)

            Spacer()

            Button {
                exportSupportBundle()
            } label: {
                Label("Export support bundle", systemImage: "square.and.arrow.up")
            }
            .keyboardShortcut("e", modifiers: [.command])
            .disabled(viewModel.lines.isEmpty && settings.providersList.isEmpty)
        }
        .padding(8)
    }

    private func copyLogsToPasteboard() {
        let text = viewModel.lines.map { line in
            "[\(line.seq)] \(line.ts) \(line.level.rawValue.uppercased()) \(line.target) — \(line.message)"
        }.joined(separator: "\n")
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }

    private func exportSupportBundle() {
        let snapshots = settings.providersList.map { provider in
            // The Swift adapter never sees the raw api_key (the backend
            // never returns it), so we always pass `nil` here unless a
            // future code path stashes it locally. The redaction policy
            // in SupportBundle.build is the durable safety net.
            SupportBundle.ProviderSnapshot(
                providerId: provider.id,
                configured: provider.configured,
                apiKey: nil,
                baseURL: nil
            )
        }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        let bundle = SupportBundle.build(
            logs: viewModel.lines,
            providers: snapshots,
            generatedAt: formatter.string(from: Date()),
            appVersion: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "dev",
            backendURL: "" // populated by the host via the HTTP base URL when wired in;
                           // in the support bundle this is informational only.
        )

        let panel = NSSavePanel()
        panel.title = "Export support bundle"
        panel.allowedContentTypes = [.plainText]
        panel.nameFieldStringValue = "harness-support-\(Int(Date().timeIntervalSince1970)).txt"
        panel.begin { response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try bundle.data(using: .utf8)?.write(to: url)
            } catch {
                NSLog("failed to write support bundle: \(error)")
            }
        }
    }
}
