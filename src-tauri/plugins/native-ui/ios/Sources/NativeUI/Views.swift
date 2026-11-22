import SwiftUI
import Tauri

@available(iOS 26.0, *)
extension View {
  /// Liquid Glass button; prominent = the selected or main one, tinted with the theme's accent.
  @ViewBuilder func glassButton(_ prominent: Bool) -> some View {
    if prominent { buttonStyle(.glassProminent) } else { buttonStyle(.glass).foregroundStyle(.primary) }
  }
}

@available(iOS 26.0, *)
struct Round: View {
  let sf: String
  let label: String?
  var on = false
  var alert = false
  let action: () -> Void

  var body: some View {
    let button = Button(action: action) { Image(systemName: sf).font(.system(size: 15, weight: .medium)).frame(width: 20, height: 20) }
      .glassButton(on)
      .buttonBorderShape(.circle)
      .accessibilityLabel(label ?? "")
    // .tint(nil) would drop the theme's accent for the system blue.
    if alert { button.tint(.orange) } else { button }
  }
}

/// Menu entries from the page; `pick` gets the entry (its `id` is an action of main.js).
@available(iOS 26.0, *)
struct MenuItems: View {
  let items: [Item]
  let pick: (Item) -> Void

  var body: some View {
    ForEach(Array(items.enumerated()), id: \.offset) { _, it in
      if it.sep == true {
        Divider()
      } else {
        Button(role: it.danger == true ? .destructive : nil) { pick(it) } label: { Label(it.l ?? "", systemImage: it.sf ?? "circle") }
          .disabled(it.disabled == true)
      }
    }
  }
}

// MARK: - Title bar

@available(iOS 26.0, *)
struct TopBar: View {
  let model: ChromeModel
  let send: Send

  var body: some View {
    let s = model.state
    // iPhone with the side panel open: its tabs take the place of the terminal tabs.
    let panelInBar = s.panel && !s.panelTabs.isEmpty
    GlassEffectContainer(spacing: 8) {
      HStack(spacing: 8) {
        Round(sf: "sidebar.left", label: s.l["sidebar"], on: s.sidebar) { send(["action": "toggle_sidebar"]) }
        if s.compact && panelInBar {
          panelPicker(s)
        } else {
          TabStrip(state: s, send: send)
          Menu {
            MenuItems(items: s.plus) { send(["action": $0.id ?? ""]) }
          } label: {
            Image(systemName: "plus").font(.system(size: 15, weight: .medium)).frame(width: 20, height: 20)
          } primaryAction: {
            send(["action": "new_tab"])
          }
          .glassButton(false)
          .buttonBorderShape(.circle)
          .accessibilityLabel(s.l["new_tab"] ?? "")
          if panelInBar { panelPicker(s).frame(maxWidth: 190) }
        }
        if s.panel && !s.panelMenu.isEmpty {
          Menu {
            MenuItems(items: s.panelMenu) { send(["action": $0.id ?? ""]) }
          } label: {
            Image(systemName: "ellipsis").font(.system(size: 15, weight: .medium)).frame(width: 20, height: 20)
          }
          .glassButton(false)
          .buttonBorderShape(.circle)
          .accessibilityLabel(s.l["panel_actions"] ?? "")
        }
        if let sync = s.sync {
          Round(sf: sync, label: s.l["sync"], alert: s.syncAlert) { send(["action": "sync"]) }
        }
        Round(sf: "sidebar.right", label: s.l["panel"], on: s.panel) { send(["action": "toggle_panel"]) }
        Round(sf: "gearshape", label: s.l["settings"]) { send(["action": "settings"]) }
      }
      .padding(.horizontal, 10)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    .tint(Color(hex: s.tint))
  }

  private func panelPicker(_ s: Chrome) -> some View {
    Picker("", selection: Binding(get: { s.panelTab }, set: { send(["action": "panel_tab:\($0)"]) })) {
      ForEach(s.panelTabs, id: \.self) { it in
        Image(systemName: it.sf ?? "circle").accessibilityLabel(it.l ?? "").tag(it.id ?? "")
      }
    }
    .pickerStyle(.segmented)
    .labelsHidden()
    .frame(maxWidth: .infinity)
  }
}

@available(iOS 26.0, *)
struct TabStrip: View {
  let state: Chrome
  let send: Send

  var body: some View {
    ScrollViewReader { proxy in
      ScrollView(.horizontal, showsIndicators: false) {
        HStack(spacing: 6) {
          ForEach(Array(state.tabs.enumerated()), id: \.offset) { i, tab in
            HStack(spacing: 4) {
              Button { send(["action": "tab:\(i)"]) } label: {
                Text(tab.l).font(.subheadline.weight(tab.on ? .semibold : .regular)).lineLimit(1).frame(maxWidth: 150)
              }
              .glassButton(tab.on)
              if tab.on {
                Button { send(["action": "close_tab:\(i)"]) } label: { Image(systemName: "xmark").font(.caption.weight(.bold)) }
                  .glassButton(false)
                  .buttonBorderShape(.circle)
                  .controlSize(.small)
                  .accessibilityLabel(state.l["close_tab"] ?? "")
              }
            }
            .id(i)
            .contextMenu {
              Button(role: .destructive) { send(["action": "close_tab:\(i)"]) } label: { Label(state.l["close_tab"] ?? "", systemImage: "xmark") }
            }
          }
        }
        .padding(.vertical, 4)
      }
      .onChange(of: state.tabs.firstIndex { $0.on }) { _, i in
        if let i { withAnimation { proxy.scrollTo(i) } }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

// MARK: - Keys above the keyboard

@available(iOS 26.0, *)
struct Key: View {
  var text = ""
  var sf: String? = nil
  var label: String? = nil
  var on = false
  var repeats = false
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      Group {
        if let sf { Image(systemName: sf) } else { Text(text) }
      }
      .frame(maxWidth: .infinity, minHeight: 24)
    }
    .glassButton(on)
    .buttonRepeatBehavior(repeats ? .enabled : .disabled)
    .accessibilityLabel(label ?? text)
  }
}

@available(iOS 26.0, *)
struct KeyBar: View {
  let model: ChromeModel
  let send: Send

  var body: some View {
    let s = model.state
    GlassEffectContainer(spacing: 6) {
      HStack(spacing: 5) {
        Key(text: "Esc") { send(["key": "esc"]) }
        Key(text: "Tab") { send(["key": "tab"]) }
        Key(text: "Ctrl", on: s.ctrl) { send(["key": "ctrl"]) }
        Key(sf: "arrow.left", label: "←", repeats: true) { send(["key": "left"]) }
        Key(sf: "arrow.up", label: "↑", repeats: true) { send(["key": "up"]) }
        Key(sf: "arrow.down", label: "↓", repeats: true) { send(["key": "down"]) }
        Key(sf: "arrow.right", label: "→", repeats: true) { send(["key": "right"]) }
        // The system paste control: no "allow paste" prompt.
        PasteButton(payloadType: String.self) { send(["key": "paste", "text": $0.joined()]) }
          .labelStyle(.iconOnly)
          .buttonBorderShape(.capsule)
        Key(sf: "keyboard.chevron.compact.down", label: s.l["kb_hide"]) { send(["key": "hide"]) }
      }
      .padding(.horizontal, 6)
      .padding(.vertical, 5)
    }
    .font(.system(size: 14, weight: .medium, design: .monospaced))
    .tint(Color(hex: s.tint))
  }
}

// MARK: - Menu at a point

@available(iOS 26.0, *)
struct MenuList: View {
  let items: [Item]
  let pick: (Int) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      ForEach(Array(items.enumerated()), id: \.offset) { i, it in
        if it.sep == true {
          Divider().padding(.vertical, 4)
        } else {
          Button { pick(i) } label: {
            Label(it.l ?? "", systemImage: it.sf ?? "circle")
              .frame(maxWidth: .infinity, alignment: .leading)
              .padding(.horizontal, 16)
              .frame(height: 40)
              .contentShape(Rectangle())
          }
          .buttonStyle(Pressed())
          .foregroundStyle(it.danger == true ? Color.red : Color.primary)
          .disabled(it.disabled == true)
          .opacity(it.disabled == true ? 0.4 : 1)
        }
      }
    }
    .padding(.vertical, 6)
    .frame(width: 250)
  }
}

struct Pressed: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.background(configuration.isPressed ? Color.primary.opacity(0.12) : .clear)
  }
}

// MARK: - Toast

@available(iOS 26.0, *)
struct ToastView: View {
  let model: ToastModel

  var body: some View {
    if model.shown {
      let error = model.kind == "error", success = model.kind == "success"
      Label(model.text, systemImage: error ? "exclamationmark.triangle.fill" : success ? "checkmark.circle.fill" : "info.circle.fill")
        .font(.subheadline)
        .lineLimit(3)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .glassEffect(.regular.tint(error ? .red.opacity(0.35) : success ? .green.opacity(0.25) : nil), in: .capsule)
        .transition(.move(edge: .top).combined(with: .opacity))
    }
  }
}
