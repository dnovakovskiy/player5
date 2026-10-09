import Foundation
import XCTest

@testable import Player5Kit

/// The `#p=` link format shared with the web app (`apps/web/src/spec.ts`).
final class ShareCodecTests: XCTestCase {
    /// What the web app writes for its default pattern:
    /// base64url(JSON.stringify(defaultSpec())), no padding.
    private let webDefaultPayload =
        "eyJicG0iOjEyNCwic2h1ZmZsZSI6MCwiYWNjZW50IjoxLCJ2b2ljZXMiOnsia2ljayI6eyJzdGVwcyI6IlgtLS14LS0tWC0tLXgtLS0iLCJ0dW5lIjowLjUsImRlY2F5IjowLjUsImxldmVsIjoxfX0sInJlbmRlciI6eyJvdXRwdXRfZ2FpbiI6MSwibGltaXRlciI6ZmFsc2V9fQ"

    func testReadsWebAppLinks() throws {
        for text in [
            "#p=" + webDefaultPayload,
            "https://example.com/player5/#p=" + webDefaultPayload,
            "  p=" + webDefaultPayload + "\n",
            webDefaultPayload,
        ] {
            let p = try XCTUnwrap(ShareCodec.pattern(from: text), text)
            XCTAssertEqual(p.bpm, 124)
            XCTAssertEqual(p.accent, 1)
            XCTAssertEqual(p.shuffle, 0)
            XCTAssertEqual(p[.kick].notation, "X---x---X---x---")
            XCTAssertEqual(p[.kick].level, 1)
            XCTAssertEqual(p.outputGain, 1)
            XCTAssertFalse(p.limiter)
        }
    }

    func testRoundTripsThroughLinks() throws {
        for preset in Presets.all {
            let hash = ShareCodec.hash(for: preset.pattern)
            XCTAssertTrue(hash.hasPrefix("#p="))
            // The web app's decoder only accepts this alphabet.
            let payload = hash.dropFirst(3)
            XCTAssertFalse(payload.isEmpty)
            XCTAssertTrue(
                payload.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") },
                String(payload))
            XCTAssertEqual(ShareCodec.pattern(from: hash), preset.pattern.normalized(), preset.name)

            let link = ShareCodec.link(for: preset.pattern, base: "https://example.com/p5/#old")
            XCTAssertEqual(link, "https://example.com/p5/" + hash)
            XCTAssertEqual(ShareCodec.pattern(from: link), preset.pattern.normalized())
        }
    }

    func testReadsPatternJSON() throws {
        let p = try XCTUnwrap(
            ShareCodec.pattern(from: #"{ "bpm": 99, "voices": { "clap": { "steps": "----X-------X---" } } }"#))
        XCTAssertEqual(p.bpm, 99)
        XCTAssertEqual(p[.clap].notation, "----X-------X---")
    }

    func testRejectsJunk() {
        XCTAssertNil(ShareCodec.pattern(from: ""))
        XCTAssertNil(ShareCodec.pattern(from: "#p="))
        XCTAssertNil(ShareCodec.pattern(from: "#p=!!!"))
        XCTAssertNil(ShareCodec.pattern(from: "hello world"))
        XCTAssertNil(ShareCodec.pattern(from: "{ not json"))
    }

    func testBase64URLAlphabet() {
        XCTAssertEqual(ShareCodec.base64URLEncode(Data([0xFB, 0xFF])), "-_8")
        XCTAssertEqual(ShareCodec.base64URLDecode("-_8"), Data([0xFB, 0xFF]))
        XCTAssertEqual(ShareCodec.base64URLDecode("-_8="), Data([0xFB, 0xFF]))
        XCTAssertNil(ShareCodec.base64URLDecode("abcde"))
    }
}
