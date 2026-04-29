import Foundation

public enum ProviderConfigError: Error, Equatable, Sendable {
    case emptyApiKey
    /// Upstream provider rejected the API key (semantic code
    /// `provider.unauthorized` or HTTP 401/403 with no semantic code).
    case unauthorized
    /// Upstream provider rate-limited us. Some endpoints carry a
    /// `Retry-After` header; the duration is surfaced when present.
    case rateLimited(retryAfter: TimeInterval?)
    /// The provider has no stored config row yet (`provider.unconfigured`).
    case unconfigured
    case transport(String)
    case server(status: Int, detail: String?)

    public var userMessage: String {
        switch self {
        case .emptyApiKey:
            return "API key is required."
        case .unauthorized:
            return "Key rejected by provider"
        case .rateLimited(let retryAfter):
            if let retryAfter, retryAfter > 0 {
                let secs = Int(retryAfter.rounded(.up))
                return "Rate limited by provider. Try again in \(secs)s."
            }
            return "Rate limited by provider. Try again shortly."
        case .unconfigured:
            return "Add an API key in the Providers tab to load models."
        case .transport(let message):
            return "Could not reach the backend: \(message)"
        case .server(let status, let detail):
            if let detail, !detail.isEmpty {
                return "Backend error (\(status)): \(detail)"
            }
            return "Backend error (\(status))."
        }
    }
}

/// Top-of-tab banner state for failures that are best surfaced as a
/// retryable banner rather than an inline form error.
public enum SettingsBannerError: Equatable, Sendable {
    case transport
    case server(status: Int)

    public var userMessage: String {
        switch self {
        case .transport:
            return "Couldn't reach the backend."
        case .server(let status):
            return "Backend error (\(status))."
        }
    }
}

@Observable
@MainActor
public final class SettingsViewModel {
    private let settings: SettingsGateway
    private let providers: ProvidersGateway

    public private(set) var appTheme: SettingsTheme
    public private(set) var currentTheme: Theme
    public private(set) var providersList: [Provider] = []
    public private(set) var models: [String: [Model]] = [:]
    public private(set) var providerError: ProviderConfigError?
    public private(set) var settingsLoadError: String?
    public private(set) var providersBannerError: SettingsBannerError?
    public private(set) var modelsErrors: [String: ProviderConfigError] = [:]
    public private(set) var isSavingProvider: Bool = false
    public private(set) var isLoadingModels: Set<String> = []
    public private(set) var isLoadingProviders: Bool = false
    /// True after the first load attempt completes. Empty-state CTAs
    /// only render once the initial load has finished, so they don't
    /// flash before the gateway responds.
    public private(set) var hasLoadedProviders: Bool = false

    public init(
        settings: SettingsGateway,
        providers: ProvidersGateway,
        initialTheme: SettingsTheme = .system
    ) {
        self.settings = settings
        self.providers = providers
        self.appTheme = initialTheme
        self.currentTheme = Self.theme(for: initialTheme)
    }

    public var providerErrorMessage: String {
        providerError?.userMessage ?? ""
    }

    public func modelsError(for providerId: String) -> ProviderConfigError? {
        modelsErrors[providerId]
    }

    public func modelsErrorMessage(for providerId: String) -> String {
        modelsErrors[providerId]?.userMessage ?? ""
    }

    /// Backwards-compatible accessor — older tests / views may still read
    /// the user-readable message directly. Prefer `modelsError(for:)` for
    /// new call sites that need the typed variant.
    public var modelsLoadError: [String: String] {
        modelsErrors.mapValues { $0.userMessage }
    }

    /// True when there is no in-flight providers load, no banner error,
    /// the providers list is empty, and we've completed at least one
    /// load attempt. Drives the "Add an API key to get started" CTA on
    /// first run.
    public var shouldShowAddProviderEmptyState: Bool {
        hasLoadedProviders
            && !isLoadingProviders
            && providersBannerError == nil
            && providersList.isEmpty
    }

    public func load() async {
        do {
            let s = try await settings.get()
            applyTheme(s.theme ?? .system)
        } catch {
            settingsLoadError = String(describing: error)
        }
        await refreshProviders()
    }

    public func refreshProviders() async {
        isLoadingProviders = true
        defer {
            isLoadingProviders = false
            hasLoadedProviders = true
        }
        do {
            providersList = try await providers.list()
            providersBannerError = nil
        } catch {
            providersBannerError = Self.bannerError(for: error)
            settingsLoadError = String(describing: error)
        }
    }

    public func setTheme(_ theme: SettingsTheme) async {
        applyTheme(theme)
        do {
            let updated = try await settings.patch(PatchSettingsRequest(theme: theme))
            applyTheme(updated.theme ?? theme)
        } catch {
            settingsLoadError = String(describing: error)
        }
    }

    public func upsertProvider(id: String, apiKey: String, baseUrl: String?) async {
        let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedKey.isEmpty else {
            providerError = .emptyApiKey
            return
        }

        let normalisedBaseURL: String? = {
            guard let baseUrl else { return nil }
            let t = baseUrl.trimmingCharacters(in: .whitespacesAndNewlines)
            return t.isEmpty ? nil : t
        }()

        let config = ProviderConfig(apiKey: trimmedKey, baseURL: normalisedBaseURL)

        providerError = nil
        isSavingProvider = true
        defer { isSavingProvider = false }

        do {
            _ = try await providers.upsertConfig(providerId: id, config)
            await refreshProviders()
        } catch {
            providerError = Self.mapError(error)
        }
    }

    public func refreshModels(providerId: String) async {
        modelsErrors.removeValue(forKey: providerId)
        isLoadingModels.insert(providerId)
        defer { isLoadingModels.remove(providerId) }
        do {
            let result = try await providers.listModels(providerId: providerId)
            models[providerId] = result
        } catch {
            modelsErrors[providerId] = Self.mapError(error)
        }
    }

    private func applyTheme(_ theme: SettingsTheme) {
        appTheme = theme
        currentTheme = Self.theme(for: theme)
    }

    private static func theme(for theme: SettingsTheme) -> Theme {
        switch theme {
        case .light: return .light
        case .dark: return .dark
        case .system: return .light
        }
    }

    private static func mapError(_ error: Error) -> ProviderConfigError {
        if let backend = error as? BackendError {
            switch backend {
            case .httpStatus(let status, let body):
                // Prefer the semantic `code` on the body (introduced by
                // server-shell's T1.followup). Fall back to status-based
                // mapping for endpoints that haven't adopted the codes.
                if let code = body?.code, code.hasPrefix("provider.") {
                    switch code {
                    case "provider.unauthorized": return .unauthorized
                    case "provider.rate_limited": return .rateLimited(retryAfter: nil)
                    case "provider.unconfigured": return .unconfigured
                    default:
                        return .server(status: status, detail: body?.detail ?? body?.title)
                    }
                }
                if status == 401 || status == 403 {
                    return .unauthorized
                }
                if status == 429 {
                    return .rateLimited(retryAfter: nil)
                }
                if status == 409 {
                    return .unconfigured
                }
                return .server(status: status, detail: body?.detail ?? body?.title)
            case .transport(let message):
                return .transport(message)
            case .decoding(let message),
                 .encoding(let message),
                 .malformedResponse(let message),
                 .malformedEvent(let message):
                return .transport(message)
            case .cancelled:
                return .transport("Request cancelled.")
            }
        }
        return .transport(String(describing: error))
    }

    private static func bannerError(for error: Error) -> SettingsBannerError {
        if let backend = error as? BackendError {
            switch backend {
            case .transport, .decoding, .encoding, .malformedResponse, .malformedEvent, .cancelled:
                return .transport
            case .httpStatus(let status, _):
                return .server(status: status)
            }
        }
        return .transport
    }
}
