import Foundation

/// Pure builder for the support-bundle text shown on the Diagnostics tab's
/// "Export support bundle" action. Stays in the Application layer (no
/// Foundation IO beyond string formatting) so the view layer can pick the
/// destination via NSSavePanel without entangling the file format here.
///
/// Redaction policy: any provider config's `apiKey` is replaced with
/// `"***"` regardless of source value. An absent (`nil`) key is omitted
/// rather than rendered as `"***"`. This is defence in depth — the server
/// today never returns the raw key over the wire — so any future code
/// path that snapshots config can call this builder safely.
public enum SupportBundle {
    public struct ProviderSnapshot: Sendable, Equatable {
        public let providerId: String
        public let configured: Bool
        public let apiKey: String?
        public let baseURL: String?

        public init(providerId: String, configured: Bool, apiKey: String?, baseURL: String?) {
            self.providerId = providerId
            self.configured = configured
            self.apiKey = apiKey
            self.baseURL = baseURL
        }
    }

    public static func build(
        logs: [LogLine],
        providers: [ProviderSnapshot],
        generatedAt: String,
        appVersion: String,
        backendURL: String
    ) -> String {
        var out = ""
        out += "Harness support bundle\n"
        out += "======================\n"
        out += "generated_at: \(generatedAt)\n"
        out += "app_version:  \(appVersion)\n"
        out += "backend_url:  \(backendURL)\n"
        out += "\n"

        out += "Providers\n"
        out += "---------\n"
        if providers.isEmpty {
            out += "(none)\n"
        } else {
            for p in providers {
                out += "- \(p.providerId)\n"
                out += "    configured: \(p.configured)\n"
                if let apiKey = p.apiKey {
                    _ = apiKey // never read; redacted unconditionally
                    out += "    api_key: \"***\"\n"
                }
                if let baseURL = p.baseURL {
                    out += "    base_url: \(baseURL)\n"
                }
            }
        }
        out += "\n"

        out += "Logs (\(logs.count) lines)\n"
        out += "-----------\n"
        if logs.isEmpty {
            out += "(empty)\n"
        } else {
            for line in logs {
                out += "[\(line.seq)] \(line.ts) \(line.level.rawValue.uppercased()) \(line.target) — \(line.message)\n"
            }
        }
        return out
    }
}
