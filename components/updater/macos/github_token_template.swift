import Foundation

enum GitHubTokenTemplateError: Error, LocalizedError {
    case invalidRepository
    case invalidURL

    var errorDescription: String? {
        switch self {
        case .invalidRepository:
            return "The update repository identity is invalid."
        case .invalidURL:
            return "The GitHub token creation link could not be constructed."
        }
    }
}

enum GitHubTokenTemplate {
    static func makeURL(for repository: String) throws -> URL {
        let parts = repository.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 2 else { throw GitHubTokenTemplateError.invalidRepository }
        let owner = parts[0]
        let name = parts[1]
        let isAlphanumeric: (UInt8) -> Bool = {
            (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
        }
        guard !owner.isEmpty, owner.count <= 39,
              !owner.hasPrefix("-"), !owner.hasSuffix("-"),
              owner.utf8.allSatisfy({ isAlphanumeric($0) || $0 == 45 }),
              !name.isEmpty, name.count <= 100, name != ".", name != "..",
              name.utf8.allSatisfy({ isAlphanumeric($0) || [45, 46, 95].contains($0) }) else {
            throw GitHubTokenTemplateError.invalidRepository
        }

        var components = URLComponents()
        components.scheme = "https"
        components.host = "github.com"
        components.path = "/settings/personal-access-tokens/new"
        components.queryItems = [
            URLQueryItem(name: "name", value: "Stainless preview updates"),
            URLQueryItem(name: "description",
                value: "Read-only Stainless updates from \(repository). Choose only this repository."),
            URLQueryItem(name: "target_name", value: String(owner)),
            URLQueryItem(name: "expires_in", value: "30"),
            URLQueryItem(name: "contents", value: "read"),
        ]
        guard let url = components.url else { throw GitHubTokenTemplateError.invalidURL }
        return url
    }
}
