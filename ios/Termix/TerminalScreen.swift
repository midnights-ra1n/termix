import SwiftUI

struct TerminalScreen: View {
    @Environment(AppModel.self) private var model
    let session: TermSession
    @State private var password = ""

    var body: some View {
        @Bindable var session = session
        @Bindable var model = model
        TerminalHost(session: session)
            .background(Color(model.theme.bg))
            .overlay { status }
            .navigationTitle(session.host.title)
            .navigationBarTitleDisplayMode(.inline)
            // Tap the title: jump to another session or open a new one, without going back to the list.
            .toolbarTitleMenu {
                ForEach(model.sessions) { s in
                    Button { model.show(s) } label: {
                        Label(s.host.title, systemImage: s === session ? "checkmark" : s.tmux == nil ? "apple.terminal" : "rectangle.stack")
                    }
                }
                Divider()
                Button("Nouvelle session sur \(session.host.title)", systemImage: "plus") { model.open(session.host) }
                Button("Connexion rapide…", systemImage: "bolt") { model.quickConnect = true }
            }
            .toolbar {
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button("Panneau du serveur", systemImage: "sidebar.right") { model.panel.toggle() }
                    Menu {
                        Button("Coller", systemImage: "doc.on.clipboard") { session.view.paste(nil) }
                            .disabled(session.phase != .open)
                        Divider()
                        Button("Agrandir le texte", systemImage: "textformat.size.larger") { model.zoom(1) }
                        Button("Réduire le texte", systemImage: "textformat.size.smaller") { model.zoom(-1) }
                        Divider()
                        if let tmux = session.tmux {
                            Text("tmux : \(tmux)")
                        }
                        Button("Fermer la session", systemImage: "xmark", role: .destructive) { model.close(session) }
                    } label: {
                        Label("Plus", systemImage: "ellipsis")
                    }
                }
            }
            .inspector(isPresented: $model.panel) {
                ServerPanel(session: session)
                    .inspectorColumnWidth(min: 300, ideal: 360, max: 480)
                    .presentationDetents([.medium, .large])
            }
            .alert("Mot de passe", isPresented: $session.askPassword) {
                SecureField("Mot de passe", text: $password)
                Button("Se connecter et mémoriser") { connect(remember: true) }
                Button("Se connecter") { connect(remember: false) }
                Button("Annuler", role: .cancel) { session.phase = .closed(nil) }
            } message: {
                Text(session.host.address)
            }
            .onChange(of: session.phase, initial: true) {
                if session.phase == .open { _ = session.view.becomeFirstResponder() }
            }
    }

    private func connect(remember: Bool) {
        let pw = password
        password = ""
        Task { await session.connect(password: pw, remember: remember) }
    }

    @ViewBuilder private var status: some View {
        switch session.phase {
        case .connecting:
            ProgressView(session.tmux == nil ? "Connexion à \(session.host.title)…" : "Reprise de la session tmux…")
                .padding(24)
                .glassEffect(.regular, in: .rect(cornerRadius: 24))
        case .closed(let err):
            VStack(spacing: 12) {
                Label(err == nil ? "Session terminée" : session.dropped ? "Connexion perdue" : "Connexion impossible",
                      systemImage: err == nil ? "powerplug" : session.dropped ? "wifi.exclamationmark" : "exclamationmark.triangle")
                    .font(.headline)
                if let err, !session.dropped { Text(err).font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center) }
                if session.tmux != nil && err != nil {
                    Text("La session tmux continue sur le serveur : se reconnecter la reprend.")
                        .font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                }
                HStack {
                    Button("Fermer") { model.close(session) }
                        .buttonStyle(.glass)
                    Button("Se reconnecter") { Task { await session.connect() } }
                        .buttonStyle(.glassProminent)
                }
            }
            .padding(24)
            .frame(maxWidth: 340)
            .glassEffect(.regular, in: .rect(cornerRadius: 24))
            .padding()
        case .open:
            EmptyView()
        }
    }
}
