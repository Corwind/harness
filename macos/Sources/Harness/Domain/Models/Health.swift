import Foundation

public struct Health: Codable, Sendable, Equatable {
    public let status: String
    public let version: String

    public init(status: String, version: String) {
        self.status = status
        self.version = version
    }
}
