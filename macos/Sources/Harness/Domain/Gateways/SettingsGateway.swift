import Foundation

public protocol SettingsGateway: Sendable {
    func get() async throws -> Settings
    func patch(_ request: PatchSettingsRequest) async throws -> Settings
}
