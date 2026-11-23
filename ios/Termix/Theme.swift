import UIKit

/// The desktop's themes (themes.json, made from src/themes.js by scripts/ios-themes.mjs) and the user's own
/// from the synced settings, resolved the same way as resolveTheme() in themes.js.
struct Theme {
    var dark: Bool
    var bg, fg, cursor, selection: UIColor
    /// nil: the system's tint ("system" theme).
    var tint: UIColor?
    var ansi: [UIColor]

    static let list: [[String: Any]] = {
        guard let url = Bundle.main.url(forResource: "themes", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let v = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return [] }
        return v
    }()
    static let builtIn: [String: [String: Any]] = Dictionary(list.map { ($0["id"] as? String ?? "", $0) }, uniquingKeysWith: { a, _ in a })

    private static let ansiKeys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
                                   "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"]

    static func resolve(_ name: String, user: [String: [String: Any]], systemDark: Bool) -> Theme {
        let fallback = builtIn["termix"] ?? [:]
        var t = user[name] ?? builtIn[name] ?? fallback
        if t["system"] as? Bool == true {
            // Same overrides as systemTheme() in themes.js, with the system tint as accent.
            let over: [String: Any] = systemDark
                ? ["bg": "#1E1E1E", "ink": "#F2F2F2", "selection": "#3A3A3A", "black": "#2E2E2E", "brightWhite": "#FFFFFF"]
                : ["bg": "#F6F6F6", "ink": "#1D1D1F", "selection": "#D6D6D6"]
            t = (builtIn[systemDark ? "nord" : "aube"] ?? fallback).merging(over) { $1 }
            var theme = Theme(t)
            theme.tint = nil
            theme.cursor = .tintColor
            return theme
        }
        if user[name] != nil {
            t = (builtIn[t["base"] as? String ?? ""] ?? fallback).merging(t) { $1 }
        }
        return Theme(t)
    }

    init(_ t: [String: Any]) {
        func color(_ keys: String...) -> UIColor {
            keys.lazy.compactMap { (t[$0] as? String).flatMap(UIColor.init(hex:)) }.first ?? .gray
        }
        dark = t["dark"] as? Bool != false
        bg = color("term_bg", "bg")
        fg = color("term_fg", "ink")
        cursor = color("cursor", "ink")
        selection = color("selection", "brume")
        // The focus colour where the accent is too dark on the background (see themes.js).
        tint = color("focus", "accent")
        ansi = Self.ansiKeys.map { color($0) }
    }
}

extension UIColor {
    /// "#RGB", "#RRGGBB" or "#RRGGBBAA".
    convenience init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        guard s.hasPrefix("#") else { return nil }
        s.removeFirst()
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        guard s.count == 6 || s.count == 8, let v = UInt64(s, radix: 16) else { return nil }
        let a = s.count == 8 ? CGFloat(v & 0xFF) / 255 : 1
        let rgb = s.count == 8 ? v >> 8 : v
        self.init(red: CGFloat((rgb >> 16) & 0xFF) / 255, green: CGFloat((rgb >> 8) & 0xFF) / 255, blue: CGFloat(rgb & 0xFF) / 255, alpha: a)
    }
}
