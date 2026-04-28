import Foundation

public enum SandboxCreateError: Error, Equatable, Sendable {
    case emptyName
    case emptyProfile
    case backend(String)

    public var userMessage: String {
        switch self {
        case .emptyName: return "Name is required."
        case .emptyProfile: return "Profile cannot be empty."
        case .backend(let m): return m
        }
    }
}

@Observable
@MainActor
public final class SandboxTemplatesViewModel {
    private let gateway: SandboxTemplatesGateway

    public private(set) var templates: [SandboxTemplate] = []
    public private(set) var loadError: String?
    public private(set) var createError: SandboxCreateError?
    public private(set) var validationResults: [String: ValidateSandboxResult] = [:]
    public private(set) var isValidating: Set<String> = []
    public private(set) var isCreating: Bool = false
    public private(set) var isLoading: Bool = false

    public init(gateway: SandboxTemplatesGateway) {
        self.gateway = gateway
    }

    public func load() async {
        loadError = nil
        isLoading = true
        defer { isLoading = false }
        do {
            templates = try await gateway.list()
        } catch {
            loadError = String(describing: error)
        }
    }

    public func canEdit(_ template: SandboxTemplate) -> Bool { !template.isBuiltin }
    public func canDelete(_ template: SandboxTemplate) -> Bool { !template.isBuiltin }

    public func createTemplate(name: String, description: String?, profile: String) async {
        createError = nil
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedProfile = profile.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty else {
            createError = .emptyName
            return
        }
        guard !trimmedProfile.isEmpty else {
            createError = .emptyProfile
            return
        }
        let trimmedDesc = description?.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = CreateSandboxTemplateRequest(
            name: trimmedName,
            description: (trimmedDesc?.isEmpty ?? true) ? nil : trimmedDesc,
            profile: profile
        )
        isCreating = true
        defer { isCreating = false }
        do {
            let created = try await gateway.create(body)
            templates.append(created)
        } catch {
            createError = .backend(String(describing: error))
        }
    }

    public func updateTemplate(id: String, name: String?, description: String?, profile: String?) async {
        guard let existing = templates.first(where: { $0.id == id }), !existing.isBuiltin else {
            return
        }
        do {
            let body = PatchSandboxTemplateRequest(name: name, description: description, profile: profile)
            let updated = try await gateway.patch(id: id, body)
            if let idx = templates.firstIndex(where: { $0.id == id }) {
                templates[idx] = updated
            }
        } catch {
            loadError = String(describing: error)
        }
    }

    public func deleteTemplate(id: String) async {
        guard let target = templates.first(where: { $0.id == id }), !target.isBuiltin else {
            return
        }
        do {
            try await gateway.delete(id: id)
            templates.removeAll(where: { $0.id == id })
            validationResults.removeValue(forKey: id)
        } catch {
            loadError = String(describing: error)
        }
    }

    public func validate(id: String) async {
        isValidating.insert(id)
        defer { isValidating.remove(id) }
        do {
            let result = try await gateway.validate(id: id)
            validationResults[id] = result
        } catch {
            validationResults[id] = ValidateSandboxResult(
                valid: false,
                stderr: "Validation request failed: \(String(describing: error))"
            )
        }
    }
}
