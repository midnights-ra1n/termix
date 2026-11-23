import SwiftUI

// Building blocks of the server panel (stats, files, Docker, tmux): one card style that reads well both on the
// glass sheet (medium detent) and on the opaque one (large detent, iPad column).

/// Fill of every card and row of the panel.
let panelFill = Color.primary.opacity(0.07)

/// A titled card.
struct PanelCard<Content: View, Accessory: View>: View {
    var title: String?
    var systemImage = ""
    @ViewBuilder var accessory: Accessory
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let title {
                HStack {
                    Label(title, systemImage: systemImage)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Spacer(minLength: 8)
                    accessory
                }
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
            }
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(panelFill, in: .rect(cornerRadius: 22))
    }
}

extension PanelCard where Accessory == EmptyView {
    init(title: String? = nil, systemImage: String = "", @ViewBuilder content: () -> Content) {
        self.init(title: title, systemImage: systemImage, accessory: { EmptyView() }, content: content)
    }
}

/// An SF Symbol in a tinted rounded square (nil colour: the theme's tint).
struct IconTile: View {
    let systemImage: String
    var color: Color?
    @ScaledMetric(relativeTo: .body) private var size = 36

    var body: some View {
        let style = color.map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint)
        Image(systemName: systemImage)
            .font(.system(size: size * 0.44, weight: .semibold))
            .foregroundStyle(style)
            .frame(width: size, height: size)
            .background(style.opacity(0.18), in: .rect(cornerRadius: size * 0.3))
            .accessibilityHidden(true)
    }
}

/// A small coloured capsule: a state or a badge. The text says it, the colour only repeats it.
struct Pill: View {
    let text: String
    var color: Color?

    var body: some View {
        let style = color.map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint)
        Text(text)
            .font(.caption2.weight(.semibold))
            .lineLimit(1)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .foregroundStyle(style)
            .background(style.opacity(0.16), in: .capsule)
    }
}

extension View {
    /// A List row drawn as a panel card.
    func panelRow() -> some View {
        listRowBackground(panelFill).listRowSeparatorTint(.primary.opacity(0.08))
    }
}

// The app's colours follow the theme like the desktop: the terminal's background everywhere, rows a shade lighter.

private struct ThemedBackground: ViewModifier {
    @Environment(AppModel.self) private var model

    func body(content: Content) -> some View {
        content.scrollContentBackground(.hidden).background(Color(model.theme.bg))
    }
}

extension View {
    /// The theme's background (the terminal's) instead of the system's black or grey.
    func themedBackground() -> some View { modifier(ThemedBackground()) }
}

/// Wraps a List or Form's sections: their rows get the panel's card fill instead of the system grey.
struct ThemedRows<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        Group { content }.listRowBackground(panelFill)
    }
}
