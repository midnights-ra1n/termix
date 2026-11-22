import SwiftUI
import Tauri

// What the page sends (src/native.js). Every field of Chrome is always sent.

/// A menu entry; a separator is {sep: true}.
struct Item: Decodable, Hashable {
  var id: String?
  var l: String?
  var sf: String?
  var danger: Bool?
  var disabled: Bool?
  var sep: Bool?
}

struct TabInfo: Decodable, Hashable {
  var l: String
  var on: Bool
}

/// Title bar and key bar state (main.js, chromeState).
struct Chrome: Decodable, Equatable {
  var tabs: [TabInfo] = []
  var sidebar = false
  var panel = false
  var compact = false
  var typing = false
  var ctrl = false
  var panelTab = ""
  var panelTabs: [Item] = []
  var panelMenu: [Item] = []
  var plus: [Item] = []
  var sync: String? = nil
  var syncAlert = false
  var tint = "#8C7CF0"
  var dark = true
  var l: [String: String] = [:]
}

/// A choice: option of a select (v, l), page tab or host icon (id of its DOM button, on).
struct Opt: Decodable, Hashable {
  var id: Int?
  var v: String?
  var l: String?
  var sf: String?
  var on: Bool?
}

/// One line of a sheet. `id` is the DOM element it mirrors; `k` its kind (RowView).
struct Row: Decodable, Identifiable {
  var id: Int
  var k: String
  var label: String?
  var hint: String?
  var text: String?
  var on: Bool?
  var placeholder: String?
  var options: [Opt]?
  var min: Double?
  var max: Double?
  var step: Double?
  var unit: String?
  var toggle: Int?
  var disabled: Bool?
  var role: String?
  var detail: String?
  var buttons: [Row]?
  var keyboard: String?
}

struct FormSection: Decodable, Identifiable {
  var id: Int
  var header: String?
  var rows: [Row]
}

struct Action: Decodable, Identifiable {
  var id: Int
  var l: String
  var primary: Bool
  var disabled: Bool?
}

struct SheetDoc: Decodable {
  var id: Int
  var title: String
  var large: Bool
  var tabs: [Opt]
  var sections: [FormSection]
  var actions: [Action]
}

@available(iOS 26.0, *)
@Observable final class ChromeModel {
  var state = Chrome()
}

@available(iOS 26.0, *)
@Observable final class SheetModel {
  var doc: SheetDoc
  /// Values edited here that the page has not echoed yet: its older copy must not undo the typing.
  private var pending: [Int: (value: String, at: Date)] = [:]

  init(_ doc: SheetDoc) { self.doc = doc }

  func value(_ row: Row) -> String {
    pending[row.id]?.value ?? row.text ?? (row.on == true ? "true" : "false")
  }

  func flag(_ id: Int, _ fallback: Bool?) -> Bool {
    pending[id].map { $0.value == "true" } ?? fallback ?? false
  }

  func edit(_ id: Int, _ value: String) { pending[id] = (value, .now) }

  func update(_ doc: SheetDoc) {
    for section in doc.sections {
      for row in section.rows {
        guard let p = pending[row.id] else { continue }
        let echoed = row.text ?? (row.on == true ? "true" : "false")
        if p.value == echoed || Date.now.timeIntervalSince(p.at) > 1.5 { pending[row.id] = nil }
      }
    }
    self.doc = doc
  }
}

@available(iOS 26.0, *)
@Observable final class ToastModel {
  var text = ""
  var kind = ""
  var shown = false
}

extension Color {
  /// "#RRGGBB" (themes and <input type=color>).
  init(hex: String) {
    var v: UInt64 = 0
    Scanner(string: hex.trimmingCharacters(in: CharacterSet(charactersIn: "# "))).scanHexInt64(&v)
    self.init(red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255)
  }

  /// Lowercase, as <input type=color> gives it back (no echo mismatch).
  var hex: String {
    var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
    UIColor(self).getRed(&r, green: &g, blue: &b, alpha: &a)
    let c = { (x: CGFloat) in Int((Swift.min(Swift.max(x, 0), 1) * 255).rounded()) }
    return String(format: "#%02x%02x%02x", c(r), c(g), c(b))
  }
}

typealias Send = (JsonObject) -> Void
