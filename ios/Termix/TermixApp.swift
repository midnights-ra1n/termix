import SwiftUI

@main
struct TermixApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView().environment(model)
        }
        .commands { SessionCommands(model: model) }
    }
}

/// Hardware keyboard (iPad): the desktop's shortcuts, also listed in the iPadOS menu bar.
private struct SessionCommands: Commands {
    let model: AppModel

    var body: some Commands {
        // Single window (no UIApplicationSupportsMultipleScenes): File → New is ours.
        CommandGroup(replacing: .newItem) {
            Button("Nouvel hôte…") { model.editing = Host() }
                .keyboardShortcut("n")
            Button("Connexion rapide…") { model.quickConnect = true }
                .keyboardShortcut("k")
        }
        CommandMenu("Session") {
            Button("Nouvelle session sur cet hôte") { if let s = model.current { model.open(s.host) } }
                .keyboardShortcut("t")
            Button("Fermer la session") { if let s = model.current { model.close(s) } }
                .keyboardShortcut("w", modifiers: [.command, .shift])
            Divider()
            Button("Session suivante") { model.cycle(1) }
                .keyboardShortcut("]")
            Button("Session précédente") { model.cycle(-1) }
                .keyboardShortcut("[")
            Divider()
            Button("Panneau du serveur") { model.panel.toggle() }
                .keyboardShortcut("i")
        }
        CommandGroup(after: .toolbar) {
            Button("Agrandir le texte") { model.zoom(1) }
                .keyboardShortcut("=")
            Button("Réduire le texte") { model.zoom(-1) }
                .keyboardShortcut("-")
            Button("Taille par défaut") { model.fontSize = 13 }
                .keyboardShortcut("0")
        }
    }
}

private struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme
    @Environment(\.scenePhase) private var phase

    var body: some View {
        @Bindable var model = model
        let theme = model.theme
        NavigationSplitView(preferredCompactColumn: $model.column) {
            HostsView()
        } detail: {
            if let s = model.current {
                TerminalScreen(session: s).id(s.id)
            } else {
                ContentUnavailableView {
                    Label("Aucune session", systemImage: "apple.terminal")
                } description: {
                    Text("Choisissez un hôte pour vous connecter.")
                } actions: {
                    Button("Connexion rapide…") { model.quickConnect = true }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(theme.bg))
            }
        }
        .tint(theme.tint.map { Color($0) })
        // The theme's light or dark look; the "system" theme follows iOS.
        .preferredColorScheme(theme.tint == nil ? nil : theme.dark ? .dark : .light)
        .onChange(of: scheme, initial: true) { model.systemDark = scheme == .dark; model.restyle() }
        .onChange(of: phase) { model.scenePhase(phase) }
        .sheet(isPresented: $model.quickConnect) { QuickConnectView() }
        .alert("Erreur", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.error ?? "")
        }
    }
}
