import SwiftUI

/// The desktop's right panel for the session's server: stats, SFTP, Docker and tmux. A column beside the terminal
/// on iPad, a sheet on iPhone.
struct ServerPanel: View {
    @Environment(AppModel.self) private var model
    let session: TermSession
    @AppStorage("panelTab") private var tab = "stats"

    var body: some View {
        VStack(spacing: 0) {
            Picker("Panneau", selection: $tab) {
                Text("Stats").tag("stats")
                Text("Fichiers").tag("files")
                Text("Docker").tag("docker")
                Text("tmux").tag("tmux")
            }
            .pickerStyle(.segmented)
            .padding(.horizontal)
            .padding(.vertical, 8)
            if session.phase == .open {
                NavigationStack {
                    switch tab {
                    case "files":
                        DirView(session: session, path: "")
                            .navigationDestination(for: String.self) { DirView(session: session, path: $0) }
                    case "docker": DockerView(session: session)
                    case "tmux": TmuxView(session: session)
                    default: StatsView(session: session)
                    }
                }
                // A new connection (reconnect) or another tab starts from the top.
                .id("\(tab)-\(session.coreId)")
            } else {
                ContentUnavailableView("Hors ligne", systemImage: "powerplug", description: Text("Le panneau revient avec la connexion."))
            }
        }
    }
}

/// tmux sessions of the server: resume one in a new session here, or end it.
struct TmuxView: View {
    @Environment(AppModel.self) private var model
    let session: TermSession
    @State private var list: [TmuxSession]?
    @State private var missing = false
    @State private var error: String?
    @State private var killing: TmuxSession?

    var body: some View {
        List {
            if let list {
                Section {
                    ForEach(list) { t in
                        let here = t.name == session.tmux
                        Button { model.open(session.host, resume: t.name) } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(t.name).font(.body.monospaced())
                                Text(detail(t, here: here)).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        .disabled(here)
                        .swipeActions {
                            Button("Terminer", systemImage: "xmark", role: .destructive) { killing = t }
                        }
                        .contextMenu {
                            Button("Reprendre dans une nouvelle session", systemImage: "play") { model.open(session.host, resume: t.name) }
                                .disabled(here)
                            Button("Terminer la session", systemImage: "xmark", role: .destructive) { killing = t }
                        }
                    }
                } footer: {
                    Text(model.tmuxDefault || session.host.tmux == "on"
                         ? "Une session tmux continue de tourner quand la connexion est perdue. Touchez-en une pour la reprendre."
                         : "Les nouvelles sessions n'utilisent pas tmux : Réglages → Terminal, ou Se connecter dans tmux sur l'hôte.")
                }
            }
        }
        .overlay {
            if missing {
                ContentUnavailableView("tmux absent", systemImage: "rectangle.stack", description: Text("tmux n'est pas installé sur ce serveur : les tâches s'arrêtent à la déconnexion."))
            } else if let error {
                ContentUnavailableView("tmux indisponible", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if list == nil {
                ProgressView()
            } else if list?.isEmpty == true {
                ContentUnavailableView("Aucune session tmux", systemImage: "rectangle.stack")
            }
        }
        .navigationTitle("tmux")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(5))
            }
        }
        .refreshable { await load() }
        .confirmationDialog("Terminer la session tmux « \(killing?.name ?? "") » ?", isPresented: Binding(get: { killing != nil }, set: { if !$0 { killing = nil } }), titleVisibility: .visible) {
            Button("Terminer", role: .destructive) {
                guard let t = killing else { return }
                Task {
                    await model.run("tmux_kill", ["id": session.coreId, "name": t.name])
                    await load()
                }
            }
        } message: {
            Text("Tout ce qui y tourne s'arrête.")
        }
    }

    private func load() async {
        do {
            let v = try await Core.call("tmux_list", ["id": session.coreId], as: [TmuxSession]?.self)
            (list, missing, error) = (v ?? [], v == nil, nil)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func detail(_ t: TmuxSession, here: Bool) -> String {
        var parts = [here ? "Cette session" : t.attached ? "Ouverte ailleurs" : nil, t.command.isEmpty ? nil : t.command,
                     "\(t.windows) fenêtre\(t.windows > 1 ? "s" : "")"].compactMap { $0 }
        if t.activity > 0 {
            parts.append(Date(timeIntervalSince1970: TimeInterval(t.activity)).formatted(.relative(presentation: .named)))
        }
        return parts.joined(separator: " · ")
    }
}

/// Containers of the server with their state, read only (like the desktop).
struct DockerView: View {
    let session: TermSession
    @State private var docker: Docker?
    @State private var error: String?

    var body: some View {
        List {
            if let docker, docker.installed {
                ForEach(docker.containers) { c in
                    HStack(spacing: 12) {
                        Circle().fill(color(c.state)).frame(width: 9, height: 9)
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(c.name).font(.body.weight(.medium))
                            Text(c.image).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1)
                            Text(c.status).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityValue(c.state)
                }
            }
        }
        .overlay {
            if let error {
                ContentUnavailableView("Docker indisponible", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if let docker {
                if !docker.installed {
                    ContentUnavailableView("Docker absent", systemImage: "shippingbox", description: Text("Docker n'est pas installé sur ce serveur."))
                } else if !docker.error.isEmpty && docker.containers.isEmpty {
                    ContentUnavailableView("Docker inaccessible", systemImage: "lock", description: Text(docker.error))
                } else if docker.containers.isEmpty {
                    ContentUnavailableView("Aucun conteneur", systemImage: "shippingbox")
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Docker")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(5))
            }
        }
        .refreshable { await load() }
    }

    private func load() async {
        do {
            docker = try await Core.call("docker_ps", ["id": session.coreId], as: Docker.self)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func color(_ state: String) -> Color {
        switch state {
        case "running": .green
        case "paused", "restarting": .orange
        case "exited", "dead": .red
        default: .gray
        }
    }
}
