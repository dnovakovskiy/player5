import Foundation

/// The web app's share format: the whole pattern lives in the URL hash as
/// `#p=<base64url(JSON)>` (no padding), see `apps/web/src/spec.ts`.
///
/// Import accepts a full link (`https://…/#p=…`), the bare hash (`#p=…`),
/// the payload alone, or plain pattern JSON (e.g. a file from `patterns/`).
public enum ShareCodec {
    /// `#p=…` for a pattern.
    public static func hash(for pattern: Pattern) -> String {
        "#p=" + base64URLEncode(Data(pattern.jsonString().utf8))
    }

    /// A shareable link: `base` (the web app's address, e.g.
    /// `https://example.github.io/player5/`) followed by the hash. With an
    /// empty `base` this is just the hash, which the web app also accepts
    /// when pasted after its address.
    public static func link(for pattern: Pattern, base: String) -> String {
        var trimmed = base.trimmingCharacters(in: .whitespacesAndNewlines)
        if let hashIndex = trimmed.firstIndex(of: "#") {
            trimmed = String(trimmed[..<hashIndex])
        }
        return trimmed + hash(for: pattern)
    }

    /// Reads a pattern from anything `link(for:base:)`, the web app or a
    /// pattern file produces. `nil` if the text is none of those.
    public static func pattern(from text: String) -> Pattern? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            return nil
        }
        if trimmed.hasPrefix("{") {
            return try? Pattern.decode(json: trimmed)
        }
        guard let payload = payload(in: trimmed),
            let data = base64URLDecode(payload)
        else {
            return nil
        }
        return try? Pattern.decode(jsonData: data)
    }

    /// The base64url payload inside a link, hash or bare payload.
    static func payload(in text: String) -> String? {
        var candidate = Substring(text)
        if let range = text.range(of: "#p=") {
            candidate = text[range.upperBound...]
        } else if text.hasPrefix("p=") {
            candidate = text.dropFirst(2)
        }
        // The payload runs to the end or to the next separator.
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        let body = candidate.prefix(while: { allowed.contains($0) })
        guard !body.isEmpty else { return nil }
        let rest = candidate.dropFirst(body.count)
        // Anything after the payload must be a separator, not more junk.
        if let next = rest.first, next != "&", next != "#", !next.isWhitespace {
            return nil
        }
        return String(body)
    }

    /// RFC 4648 §5 base64url without padding (what the web app writes).
    public static func base64URLEncode(_ data: Data) -> String {
        var s = data.base64EncodedString()
        s = s.replacingOccurrences(of: "+", with: "-")
        s = s.replacingOccurrences(of: "/", with: "_")
        while s.hasSuffix("=") {
            s.removeLast()
        }
        return s
    }

    /// Decodes base64url with or without padding.
    public static func base64URLDecode(_ text: String) -> Data? {
        var s = text.replacingOccurrences(of: "-", with: "+")
        s = s.replacingOccurrences(of: "_", with: "/")
        let remainder = s.count % 4
        if remainder == 1 {
            return nil
        }
        if remainder > 0 {
            s += String(repeating: "=", count: 4 - remainder)
        }
        return Data(base64Encoded: s)
    }
}
