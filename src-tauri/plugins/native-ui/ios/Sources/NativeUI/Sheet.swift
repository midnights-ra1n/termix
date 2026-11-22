import SwiftUI
import Tauri

/// A dialog of the page (settings, host editor…) as a native form. Edits and taps go back to the page,
/// which applies them to its own DOM controls and sends the updated sheet.
@available(iOS 26.0, *)
struct SheetView: View {
  let model: SheetModel
  let tint: Color
  let send: Send

  var body: some View {
    let d = model.doc
    let primary = d.actions.first { $0.primary }
    let cancel = d.actions.first { !$0.primary }
    let others = d.actions.filter { !$0.primary && $0.id != cancel?.id }
    NavigationStack {
      Form {
        ForEach(d.sections) { section in
          Section {
            ForEach(section.rows) { RowView(row: $0, model: model, send: send) }
          } header: {
            if let h = section.header { Text(h) }
          }
        }
        if !others.isEmpty {
          Section {
            ForEach(others) { a in Button(a.l) { send(["click": a.id]) }.disabled(a.disabled == true) }
          }
        }
      }
      .font(.subheadline) // dense, like the desktop dialogs
      .environment(\.defaultMinListRowHeight, 34)
      .listSectionSpacing(.compact)
      .navigationTitle(d.title)
      .navigationBarTitleDisplayMode(.inline)
      .safeAreaInset(edge: .top, spacing: 0) {
        if !d.tabs.isEmpty { PageTabs(tabs: d.tabs, send: send) }
      }
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .cancel) {
            if let cancel { send(["click": cancel.id]) } else { send(["close": true]) }
          } label: {
            Label(cancel?.l ?? "OK", systemImage: "xmark")
          }
        }
        if let primary {
          ToolbarItem(placement: .confirmationAction) {
            Button { send(["click": primary.id]) } label: { Label(primary.l, systemImage: "checkmark") }
              .buttonStyle(.glassProminent)
              .disabled(primary.disabled == true)
          }
        }
      }
    }
    .tint(tint)
  }
}

/// Pages of a dialog (Settings): a row of glass capsules under the title.
@available(iOS 26.0, *)
struct PageTabs: View {
  let tabs: [Opt]
  let send: Send

  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      GlassEffectContainer(spacing: 6) {
        HStack(spacing: 6) {
          ForEach(tabs, id: \.self) { t in
            Button(t.l ?? "") { send(["click": t.id ?? 0]) }
              .glassButton(t.on == true)
              .controlSize(.small)
          }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
      }
    }
  }
}

@available(iOS 26.0, *)
struct RowView: View {
  let row: Row
  let model: SheetModel
  let send: Send

  /// The row's value: typed here first, then sent to the page.
  private var value: Binding<String> {
    Binding(get: { model.value(row) }, set: { v in
      model.edit(row.id, v)
      send(["change": row.id, "value": v])
    })
  }

  private var prompt: Text? { row.placeholder.flatMap { $0.isEmpty ? nil : Text($0) } }

  var body: some View {
    let label = row.label ?? ""
    switch row.k {
    case "note":
      Text(label).font(.footnote).foregroundStyle(.secondary)
    case "error":
      Text(label).font(.footnote).foregroundStyle(.red)
    case "title":
      Text(label).font(.title3.weight(.semibold)).frame(maxWidth: .infinity)
    case "pair":
      LabeledContent(label) { Text(row.text ?? "").font(.footnote.monospaced()).textSelection(.enabled) }
    case "select":
      Picker(selection: value) {
        ForEach(row.options ?? [], id: \.self) { Text($0.l ?? "").tag($0.v ?? "") }
      } label: {
        Titled(title: label, hint: row.hint)
      }
      .pickerStyle(.menu)
      .disabled(row.disabled == true)
    case "toggle":
      Toggle(isOn: Binding(get: { value.wrappedValue == "true" }, set: { value.wrappedValue = $0 ? "true" : "false" })) {
        Titled(title: label, hint: row.hint)
      }
      .disabled(row.disabled == true)
    case "slider":
      slider
    case "color":
      color
    case "number":
      number
    case "multiline":
      VStack(alignment: .leading, spacing: 4) {
        Text(label)
        TextField("", text: value, prompt: prompt, axis: .vertical).lineLimit(2...6)
      }
    case "field", "secure":
      field
    case "button":
      Button(role: row.role == "danger" ? .destructive : nil) { send(["click": row.id]) } label: { Text(label) }
        .glassButton(row.role == "primary")
        .disabled(row.disabled == true)
    case "icons":
      icons
    case "item":
      item
    default:
      EmptyView()
    }
  }

  @ViewBuilder private var hint: some View {
    if let h = row.hint, !h.isEmpty { Text(h).font(.caption).foregroundStyle(.secondary) }
  }

  @ViewBuilder private var input: some View {
    Group {
      if row.k == "secure" {
        SecureField("", text: value, prompt: prompt)
      } else {
        TextField("", text: value, prompt: prompt)
          .keyboardType(row.keyboard == "numeric" ? .numberPad : row.keyboard == "url" ? .URL : .default)
      }
    }
    .textInputAutocapitalization(.never)
    .autocorrectionDisabled()
  }

  private var field: some View {
    VStack(alignment: .leading, spacing: 2) {
      if (row.label ?? "").isEmpty {
        input
      } else {
        LabeledContent { input.multilineTextAlignment(.trailing) } label: { Text(row.label ?? "") }
      }
      hint
    }
    .disabled(row.disabled == true)
  }

  private var number: some View {
    let lo = row.min ?? -1e9, hi = Swift.max(row.max ?? 1e9, lo), step = row.step ?? 1
    let num = Binding<Double>(get: { Double(value.wrappedValue) ?? lo }, set: { value.wrappedValue = format($0, step) })
    return VStack(alignment: .leading, spacing: 2) {
      LabeledContent {
        HStack(spacing: 8) {
          TextField("", text: value, prompt: prompt)
            .keyboardType(.decimalPad)
            .multilineTextAlignment(.trailing)
            .monospacedDigit()
            .frame(maxWidth: 80)
          Stepper("", value: num, in: lo...hi, step: step).labelsHidden()
        }
      } label: {
        Text(row.label ?? "")
      }
      hint
    }
  }

  private var slider: some View {
    let lo = row.min ?? 0, hi = Swift.max(row.max ?? 100, lo + 1), step = row.step ?? 1
    let num = Binding<Double>(get: { Double(value.wrappedValue) ?? lo }, set: { value.wrappedValue = format($0, step) })
    return VStack(alignment: .leading, spacing: 2) {
      HStack {
        Text(row.label ?? "")
        Spacer()
        Text(value.wrappedValue + (row.unit ?? "")).monospacedDigit().foregroundStyle(.secondary)
      }
      Slider(value: num, in: lo...hi, step: step)
      hint
    }
  }

  private var color: some View {
    HStack {
      ColorPicker(selection: Binding(get: { Color(hex: value.wrappedValue) }, set: { value.wrappedValue = $0.hex }), supportsOpacity: false) {
        Text(row.label ?? "")
      }
      if let toggle = row.toggle {
        Toggle("", isOn: Binding(get: { model.flag(toggle, row.on) }, set: { on in
          model.edit(toggle, on ? "true" : "false")
          send(["change": toggle, "value": on ? "true" : "false"])
        }))
        .labelsHidden()
      }
    }
  }

  private var icons: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(row.label ?? "")
      LazyVGrid(columns: [GridItem(.adaptive(minimum: 40), spacing: 6)], spacing: 6) {
        ForEach(row.options ?? [], id: \.self) { o in
          Button { send(["click": o.id ?? 0]) } label: { Image(systemName: o.sf ?? "server.rack").frame(width: 20, height: 20) }
            .glassButton(o.on == true)
            .buttonBorderShape(.circle)
            .accessibilityLabel(o.l ?? "")
        }
      }
    }
  }

  private var item: some View {
    HStack(spacing: 8) {
      VStack(alignment: .leading, spacing: 2) {
        Text(row.label ?? "").font(.subheadline.weight(.medium))
        if let d = row.detail, !d.isEmpty { Text(d).font(.caption).foregroundStyle(.secondary).lineLimit(6) }
      }
      Spacer(minLength: 4)
      ForEach(row.buttons ?? []) { b in
        Button(b.label ?? "") { send(["click": b.id]) }
          .glassButton(b.role == "primary")
          .controlSize(.small)
          .disabled(b.disabled == true)
      }
    }
  }
}

@available(iOS 26.0, *)
struct Titled: View {
  let title: String
  let hint: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(title)
      if let hint, !hint.isEmpty { Text(hint).font(.caption).foregroundStyle(.secondary) }
    }
  }
}

/// Whole numbers stay whole ("12", not "12.0"): the page reads them back as typed values.
private func format(_ v: Double, _ step: Double) -> String {
  step.rounded() == step ? String(Int(v.rounded())) : String(format: "%g", v)
}
