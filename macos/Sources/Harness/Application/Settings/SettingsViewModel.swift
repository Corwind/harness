import Foundation

public enum ProviderConfigError: Error, Equatable, Sendable {
    case emptyApiKey
    case unauthorized
    case transport(String)
    case server(status: Int, detail: String?)

    public var userMessage: String {
        switch self {
        case .emptyApiKey:
            return "API key is required."
        case .unauthorized:
            return "The API key was rejected. Double-check it and save again."
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
    public private(set) var modelsLoadError: [String: String] = [:]
    public private(set) var isSavingProvider: Bool = false
    public private(set) var isLoadingModels: Set<String> = []

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
        do {
            providersList = try await providers.list()
        } catch {
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
        modelsLoadError[providerId] = nil
        isLoadingModels.insert(providerId)
        defer { isLoadingModels.remove(providerId) }
        do {
            let result = try await providers.listModels(providerId: providerId)
            models[providerId] = result
        } catch {
            modelsLoadError[providerId] = Self.mapError(error).userMessage
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
                if status == 401 || status == 403 {
                    return .unauthorized
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
}
