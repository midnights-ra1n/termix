import SwiftUI

/// The desktop's right panel for the session's server: stats, SFTP, Docker and tmux. A column beside the terminal
/// on iPad, a sheet on iPhone.
struct ServerPanel: View {
    @Environment(AppModel.self) private var model
    let session: TermSession
    @AppStorage("panelTab") private var tab = PanelTab.stats

    enum PanelTab: String, CaseIterable {
        case stats, files, docker, tmux
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            if session.phase == .open {
                // The system's tab bar: Liquid Glass, slide the finger across it to switch, VoiceOver for free.
                // Each tab keeps its state (open folder, scroll) while the others are shown.
                TabView(selection: $tab) {
                    Tab("Stats", systemImage: "gauge.with.dots.needle.50percent", value: .stats) {
                        StatsView(session: session)
                    }
                    // Only Files (folders push) and Docker (search field) have a navigation bar. Hiding one from
                    // in here would also hide the terminal's own bar (title, back, menus).
                    Tab("Fichiers", systemImage: "folder", value: .files) {
                        NavigationStack {
                            DirView(session: session, path: "")
                                .navigationDestination(for: String.self) { DirView(session: session, path: $0) }
                        }
                    }
                    Tab("Docker", systemImage: "shippingbox", value: .docker) {
                        NavigationStack { DockerView(session: session) }
                    }
                    Tab("tmux", systemImage: "rectangle.stack", value: .tmux) {
                        TmuxView(session: session)
                    }
                }
                .tabViewStyle(.tabBarOnly)
                // A new connection (reconnect) starts every tab over.
                .id(session.coreId)
            } else {
                ContentUnavailableView("Hors ligne", systemImage: "powerplug", description: Text("Le panneau revient avec la connexion."))
                    .frame(maxHeight: .infinity)
            }
        }
        .themedBackground()
    }

    private var header: some View {
        let online = session.phase == .open
        return HStack(spacing: 12) {
            IconTile(systemImage: session.host.symbol)
            VStack(alignment: .leading, spacing: 2) {
                Text(session.host.title).font(.headline).lineLimit(1)
                HStack(spacing: 6) {
                    Circle().fill(online ? Color.green : Color.secondary).frame(width: 7, height: 7)
                    Text(session.host.address).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            if let tmux = session.tmux {
                Pill(text: "tmux · \(tmux)")
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 18)
        .padding(.bottom, 12)
        .accessibilityElement(children: .combine)
        .accessibilityValue(online ? "Connecté" : "Hors ligne")
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
            if let list, !list.isEmpty {
                Section {
                    ForEach(list) { t in
                        let here = t.name == session.tmux
                        Button { model.open(session.host, resume: t.name) } label: { row(t, here: here) }
                            .disabled(here)
                            .panelRow()
                            .swipeActions {
                                Button("Terminer", systemImage: "xmark", role: .destructive) { killing = t }
                            }
                            .contextMenu {
                                Button("Reprendre dans une nouvelle session", systemImage: "play") { model.open(session.host, resume: t.name) }
                                    .disabled(here)
                                Button("Terminer la session", systemImage: "xmark", role: .destructive) { killing = t }
                            }
                    }
                } header: {
                    Text("\(list.count) session\(list.count > 1 ? "s" : "")")
                } footer: {
                    Text(model.tmuxDefault || session.host.tmux == "on"
                         ? "Une session tmux continue de tourner quand la connexion est perdue. Touchez-en une pour la reprendre, balayez pour la terminer."
                         : "Les nouvelles sessions n'utilisent pas tmux : Réglages → Terminal, ou Se connecter dans tmux sur l'hôte.")
                }
            }
        }
        .themedBackground()
        .contentMargins(.top, 0, for: .scrollContent)
        .overlay {
            if missing {
                ContentUnavailableView("tmux absent", systemImage: "rectangle.stack", description: Text("tmux n'est pas installé sur ce serveur : les tâches s'arrêtent à la déconnexion."))
            } else if let error {
                ContentUnavailableView("tmux indisponible", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if list == nil {
                ProgressView()
            } else if list?.isEmpty == true {
                ContentUnavailableView("Aucune session tmux", systemImage: "rectangle.stack", description: Text("Les sessions ouvertes dans tmux sur ce serveur apparaissent ici."))
            }
        }
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

    private func row(_ t: TmuxSession, here: Bool) -> some View {
        HStack(spacing: 12) {
            IconTile(systemImage: here ? "checkmark.rectangle.stack" : "rectangle.stack", color: here ? nil : t.attached ? .orange : .secondary)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(t.name).font(.body.monospaced().weight(.semibold)).lineLimit(1)
                    if here {
                        Pill(text: "Ici")
                    } else if t.attached {
                        Pill(text: "Ouverte ailleurs", color: .orange)
                    }
                }
                Text(detail(t)).font(.caption).foregroundStyle(.secondary).lineLimit(2)
            }
            Spacer(minLength: 4)
            if !here {
                Image(systemName: "arrow.up.forward.app").foregroundStyle(.tint).accessibilityHidden(true)
            }
        }
        .padding(.vertical, 4)
        .foregroundStyle(.primary)
        .accessibilityElement(children: .combine)
        .accessibilityHint(here ? "" : "Reprend la session dans un nouvel onglet")
    }

    private func load() async {
        do {
            let v = try await Core.call("tmux_list", ["id": session.coreId], as: [TmuxSession]?.self)
            (list, missing, error) = (v ?? [], v == nil, nil)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func detail(_ t: TmuxSession) -> String {
        var parts = [t.command.isEmpty ? nil : t.command, "\(t.windows) fenêtre\(t.windows > 1 ? "s" : "")"].compactMap { $0 }
        if t.activity > 0 {
            parts.append(Date(timeIntervalSince1970: TimeInterval(t.activity)).formatted(.relative(presentation: .named)))
        }
        return parts.joined(separator: " · ")
    }
}

/// Containers of the server with their state, read only (like the desktop), grouped by state.
struct DockerView: View {
    let session: TermSession
    @State private var docker: Docker?
    @State private var error: String?
    @State private var query = ""

    private struct Group: Identifiable {
        let id: String, title: String, color: Color
        let items: [Docker.Container]
    }

    var body: some View {
        List {
            if let docker, docker.installed, !docker.containers.isEmpty {
                Section { summary(docker.containers) }
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                ForEach(groups(docker.containers)) { g in
                    Section {
                        ForEach(g.items) { row($0, color: g.color).panelRow() }
                    } header: {
                        Text("\(g.title) · \(g.items.count)")
                    }
                }
            }
        }
        .themedBackground()
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .automatic), prompt: "Nom ou image")
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
                } else if !query.isEmpty && groups(docker.containers).isEmpty {
                    ContentUnavailableView.search(text: query)
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

    private func summary(_ all: [Docker.Container]) -> some View {
        let running = all.filter { $0.state == "running" }.count
        let stopped = all.filter { ["exited", "dead"].contains($0.state) }.count
        return HStack(spacing: 10) {
            count(running, "En marche", .green)
            count(stopped, "Arrêtés", .red)
            count(all.count - running - stopped, "Autres", .orange)
        }
    }

    private func count(_ n: Int, _ label: String, _ color: Color) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("\(n)").font(.title2.weight(.bold)).foregroundStyle(n > 0 ? color : .secondary)
            Text(label).font(.caption.weight(.medium)).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(panelFill, in: .rect(cornerRadius: 18))
        .accessibilityElement(children: .combine)
    }

    private func groups(_ all: [Docker.Container]) -> [Group] {
        let q = query.trimmingCharacters(in: .whitespaces)
        let shown = q.isEmpty ? all : all.filter { $0.name.localizedCaseInsensitiveContains(q) || $0.image.localizedCaseInsensitiveContains(q) }
        let sorted = shown.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        return [
            Group(id: "up", title: "En marche", color: .green, items: sorted.filter { $0.state == "running" }),
            Group(id: "other", title: "En pause ou redémarrage", color: .orange, items: sorted.filter { !["running", "exited", "dead"].contains($0.state) }),
            Group(id: "down", title: "Arrêtés", color: .red, items: sorted.filter { ["exited", "dead"].contains($0.state) }),
        ].filter { !$0.items.isEmpty }
    }

    private func row(_ c: Docker.Container, color: Color) -> some View {
        HStack(spacing: 12) {
            IconTile(systemImage: c.state == "running" ? "shippingbox.fill" : "shippingbox", color: color)
            VStack(alignment: .leading, spacing: 3) {
                Text(c.name).font(.body.weight(.semibold)).lineLimit(1)
                Text(c.image).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Text(c.status).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            Pill(text: state(c.state), color: color)
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .contextMenu {
            Button("Copier le nom", systemImage: "doc.on.doc") { UIPasteboard.general.string = c.name }
            Button("Copier l'image", systemImage: "doc.on.doc") { UIPasteboard.general.string = c.image }
        }
    }

    private func state(_ s: String) -> String {
        ["running": "En marche", "exited": "Arrêté", "dead": "Mort", "paused": "En pause", "restarting": "Redémarre", "created": "Créé"][s] ?? s
    }

    private func load() async {
        do {
            docker = try await Core.call("docker_ps", ["id": session.coreId], as: Docker.self)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}
