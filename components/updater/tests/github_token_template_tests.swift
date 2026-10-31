import Foundation

// This offline harness never opens GitHub or creates credentials.
private func expect(_ condition: Bool, _ message: String) throws {
    if !condition {
        throw NSError(domain: "StainlessTokenTemplateTests", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}

@main
struct GitHubTokenTemplateTests {
    static func main() throws {
        let repository = "synthetic-owner/fixture.repo_1"
        let url = try GitHubTokenTemplate.makeURL(for: repository)
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let items = components.queryItems else {
            throw NSError(domain: "StainlessTokenTemplateTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Token template has no query fields"])
        }
        try expect(components.scheme == "https" && components.host == "github.com",
            "Token creation must stay on GitHub HTTPS")
        try expect(components.path == "/settings/personal-access-tokens/new",
            "Token creation must use the fine-grained token form")
        var parameters: [String: String] = [:]
        for item in items {
            guard let value = item.value else {
                throw NSError(domain: "StainlessTokenTemplateTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Token template has an empty query value"])
            }
            try expect(parameters.updateValue(value, forKey: item.name) == nil,
                "Token template has duplicate query fields")
        }
        try expect(Set(parameters.keys) == ["name", "description", "target_name", "expires_in", "contents"],
            "Token template includes unsupported fields or additional permissions")
        try expect(parameters["target_name"] == "synthetic-owner",
            "Token template selected the wrong resource owner")
        try expect(parameters["contents"] == "read" && parameters["expires_in"] == "30",
            "Token template is not read-only with a bounded expiration")
        try expect(parameters["name"] == "Stainless preview updates"
            && parameters["description"]?.contains(repository) == true,
            "Token template lost its intended name or repository instructions")

        for invalid in ["", "owner", "/repo", "owner/", "owner/repo/extra",
            "owner/repo?contents=write", "owner/..", "-owner/repo", "own er/repo"] {
            do {
                _ = try GitHubTokenTemplate.makeURL(for: invalid)
                throw NSError(domain: "StainlessTokenTemplateTests", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "Invalid repository identity was accepted"])
            } catch GitHubTokenTemplateError.invalidRepository {
                // Expected: repository input cannot inject fields or change the token form.
            }
        }
        print("GitHub fine-grained token template checks passed.")
    }
}
