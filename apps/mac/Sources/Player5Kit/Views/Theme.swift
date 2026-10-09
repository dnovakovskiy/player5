import Foundation
import SwiftUI

/// Colours and sizes shared with the web app (`apps/web/src/style.css`):
/// dark, high contrast, orange accent, large touch targets.
public enum Theme {
    public static let background = Color(hex: 0x0F1115)
    public static let panel = Color(hex: 0x171A21)
    public static let line = Color(hex: 0x262A33)
    public static let control = Color(hex: 0x222632)
    public static let text = Color(hex: 0xE8E8E8)
    public static let muted = Color(hex: 0x8B90A0)
    public static let accent = Color(hex: 0xFF7A1A)
    public static let hit = Color(hex: 0xFFB060)
    public static let playhead = Color(hex: 0x6CF0FF)
    public static let stepOff = Color(hex: 0x1D212A)
    public static let beatLine = Color(hex: 0x3A4050)
    public static let good = Color(hex: 0x7EE787)
    public static let warning = Color(hex: 0xFF6B6B)

    /// Step cell edge; 44 pt is the minimum comfortable touch target.
    public static let cell: CGFloat = 44
    public static let cellSpacing: CGFloat = 5
    public static let corner: CGFloat = 8

    static func mono(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
}

/// Readouts. Kept off the `View` types so they carry no actor isolation and
/// can be passed around as plain functions.
enum Formatting {
    /// `0...1` as a percentage.
    static func percent(_ value: Double) -> String {
        guard value.isFinite else { return "–" }
        return "\(Int((value * 100).rounded()))%"
    }

    /// Linear gain in dB (1 = 0 dB).
    static func decibels(_ gain: Double) -> String {
        guard gain.isFinite, gain > 0.000_01 else { return "−∞ dB" }
        return String(format: "%+.1f dB", 20 * log10(gain))
    }

    /// BPM without trailing zeros for whole tempos.
    static func bpm(_ bpm: Double) -> String {
        guard bpm.isFinite else { return "" }
        if bpm == bpm.rounded() {
            return String(format: "%.0f", bpm)
        }
        return String(format: "%.2f", bpm)
    }
}

extension Color {
    /// `0xRRGGBB` in sRGB.
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1)
    }
}

/// A titled card, like the web app's `<section>`s.
struct Panel<Content: View>: View {
    let title: String
    let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title.uppercased())
                .font(Theme.mono(12, .semibold))
                .tracking(2)
                .foregroundStyle(Theme.muted)
            content
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(Theme.line, lineWidth: 1)
        )
    }
}

/// A large, high-contrast button.
struct BoothButtonStyle: ButtonStyle {
    var prominent = false
    var minWidth: CGFloat = 44

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.mono(15, .semibold))
            .foregroundStyle(prominent ? Color.black : Theme.text)
            .padding(.horizontal, 12)
            .frame(minWidth: minWidth, minHeight: 44)
            .background(
                RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                    .fill(prominent ? Theme.accent : Theme.control)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                    .stroke(prominent ? Color.clear : Theme.line, lineWidth: 1)
            )
            .opacity(configuration.isPressed ? 0.7 : 1)
            .contentShape(Rectangle())
    }
}

/// A `0...1` (or other range) slider with a label and a value readout.
struct LabeledSlider: View {
    let title: String
    @Binding var value: Double
    var range: ClosedRange<Double> = 0...1
    var format: (Double) -> String = Formatting.percent

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title)
                    .foregroundStyle(Theme.muted)
                Spacer()
                Text(format(value))
                    .monospacedDigit()
                    .foregroundStyle(Theme.text)
            }
            .font(Theme.mono(12))
            Slider(value: $value, in: range)
                .accessibilityLabel(title)
                .accessibilityValue(format(value))
        }
        .frame(minWidth: 120)
    }
}
