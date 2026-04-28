import Foundation

public protocol HealthGateway: Sendable {
    func getHealth() async throws -> Health
}
