import SwiftUI

struct HostsView: View {
    @Environment(AppModel.self) private var model
    @State private var query = ""
    @State private var tags: Set<String> = []
    @State private var settings = false
    @State private var folderName = ""

    private var allTags: [String] { Array(Set(model.doc.hosts.flatMap(\.tags))).sorted() }

    /// Search or tag filter: a flat list instead of the tree.
    private var filtered: [Host]? {
        if query.isEmpty && tags.isEmpty { return nil }
        let q = query.lowercased()
        return model.doc.hosts.filter { h in
            tags.isSubset(of: h.tags)
                && (q.isEmpty || [h.name, h.host, h.user, h.description, h.tags.joined(separator: " ")].contains { $0.lowercased().contains(q) })
        }.sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
    }

    var body: some View {
        @Bindable var model = model
        List {
            if !model.sessions.isEmpty {
                Section("Sessions") {
                    ForEach(model.sessions) { s in SessionRow(session: s) }
                }
            }
            if let filtered {
                Section { ForEach(filtered) { HostRow(host: $0) } }
            } else {
                // The computer's open tabs (with their tmux sessions), to carry on here.
                if !model.workspaces.isEmpty {
                    Section("Sur vos autres appareils") {
                        ForEach(model.workspaces) { ws in
                            Button { model.open(ws) } label: {
                                Label {
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text("Reprendre les sessions de \(ws.name)").foregroundStyle(.primary)
                                        Text(ws.panes.compactMap { p in model.doc.hosts.first { $0.id == p.host }?.title }.joined(separator: ", "))
                                            .font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                                    }
                                } icon: {
                                    Image(systemName: "macbook.and.iphone")
                                }
                            }
                        }
                    }
                }
                let recent = Array(model.recentHosts.prefix(3))
                if !recent.isEmpty {
                    Section("Récents") { ForEach(recent) { HostRow(host: $0) } }
                }
                let favorites = model.doc.hosts.filter(\.favorite)
                if !favorites.isEmpty {
                    Section("Favoris") { ForEach(favorites) { HostRow(host: $0) } }
                }
                Section("Hôtes") { FolderContent(parent: nil) }
            }
        }
        .overlay {
            if model.doc.hosts.isEmpty && model.doc.folders.isEmpty {
                ContentUnavailableView {
                    Label("Aucun hôte", systemImage: "server.rack")
                } description: {
                    Text("Ajoutez un hôte, ou retrouvez ceux de votre ordinateur avec la synchronisation.")
                } actions: {
                    Button("Nouvel hôte") { model.editing = Host() }
                    Button("Synchronisation") { settings = true }
                }
            } else if filtered?.isEmpty == true {
                ContentUnavailableView.search(text: query)
            }
        }
        .searchable(text: $query, prompt: "Nom, adresse, tag…")
        .refreshable {
            await model.run("sync_now")
            await model.probe()
        }
        // Status dots: one TCP connection per host, while the list is on screen.
        .task {
            while !Task.isCancelled {
                await model.probe()
                try? await Task.sleep(for: .seconds(30))
            }
        }
        .navigationTitle("Termix")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button("Réglages", systemImage: "gearshape") { settings = true }
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                if !allTags.isEmpty {
                    Menu {
                        ForEach(allTags, id: \.self) { t in
                            Toggle(t, isOn: Binding(get: { tags.contains(t) }, set: { on in if on { tags.insert(t) } else { tags.remove(t) } }))
                        }
                        if !tags.isEmpty {
                            Divider()
                            Button("Tout afficher") { tags = [] }
                        }
                    } label: {
                        Label("Tags", systemImage: tags.isEmpty ? "tag" : "tag.fill")
                    }
                }
                Menu {
                    Button("Connexion rapide…", systemImage: "bolt") { model.quickConnect = true }
                    Button("Nouvel hôte", systemImage: "plus") { model.editing = Host() }
                    Button("Nouveau dossier", systemImage: "folder.badge.plus") { model.naming = Folder() }
                } label: {
                    Label("Ajouter", systemImage: "plus")
                }
            }
        }
        .sheet(item: $model.editing) { HostEditor(host: $0) }
        .sheet(isPresented: $settings) { SettingsView() }
        .alert(model.naming.map { f in model.doc.folders.contains { $0.id == f.id } ? "Renommer le dossier" : "Nouveau dossier" } ?? "",
               isPresented: Binding(get: { model.naming != nil }, set: { if !$0 { model.naming = nil } })) {
            TextField("Nom", text: $folderName)
            Button("OK") { saveFolder() }
            Button("Annuler", role: .cancel) {}
        }
        .onChange(of: model.naming?.id) { folderName = model.naming?.name ?? "" }
    }

    private func saveFolder() {
        guard var f = model.naming, !folderName.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        f.name = folderName.trimmingCharacters(in: .whitespaces)
        if let i = model.doc.folders.firstIndex(where: { $0.id == f.id }) { model.doc.folders[i] = f } else { model.doc.folders.append(f) }
        model.saveHosts()
    }
}

/// A folder level of the tree: subfolders (collapsible), then its hosts.
private struct FolderContent: View {
    @Environment(AppModel.self) private var model
    let parent: String?

    var body: some View {
        let byName = { (a: String, b: String) in a.localizedStandardCompare(b) == .orderedAscending }
        ForEach(model.doc.folders.filter { $0.parent == parent }.sorted { byName($0.name, $1.name) }) { f in
            DisclosureGroup {
                FolderContent(parent: f.id)
            } label: {
                Label(f.name, systemImage: "folder")
                    .contextMenu {
                        Button("Nouvel hôte ici", systemImage: "plus") { model.editing = Host(folder: f.id) }
                        Button("Nouveau sous-dossier", systemImage: "folder.badge.plus") { model.naming = Folder(parent: f.id) }
                        Button("Renommer", systemImage: "pencil") { model.naming = f }
                        Divider()
                        Button("Supprimer le dossier", systemImage: "trash", role: .destructive) { model.delete(f) }
                    }
            }
        }
        // At the root, also hosts whose folder no longer exists.
        ForEach(model.doc.hosts.filter { h in h.folder == parent || (parent == nil && !model.doc.folders.contains { $0.id == h.folder }) }
            .sorted { byName($0.title, $1.title) }) { HostRow(host: $0) }
    }
}

struct HostRow: View {
    @Environment(AppModel.self) private var model
    let host: Host
    @State private var confirmDelete = false

    var body: some View {
        Button { model.open(host) } label: {
            HStack(spacing: 12) {
                Image(systemName: host.symbol)
                    .font(.title3)
                    .foregroundStyle(.tint)
                    .frame(width: 30)
                    .overlay(alignment: .bottomTrailing) {
                        if let st = model.status[host.id] {
                            Circle().fill(st == "up" ? .green : st == "ping" ? .yellow : .red)
                                .frame(width: 9, height: 9)
                                .overlay { Circle().strokeBorder(Color(.systemBackground), lineWidth: 1.5) }
                                .offset(x: 3, y: 3)
                        }
                    }
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 4) {
                        Text(host.title).font(.body.weight(.medium))
                        if host.favorite {
                            Image(systemName: "star.fill").font(.caption2).foregroundStyle(.yellow)
                        }
                    }
                    Text(host.address).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                    if !host.tags.isEmpty {
                        HStack(spacing: 4) {
                            ForEach(host.tags, id: \.self) { t in
                                Text(t).font(.caption2).padding(.horizontal, 6).padding(.vertical, 1)
                                    .background(.tint.opacity(0.15), in: .capsule)
                            }
                        }
                    }
                }
                .foregroundStyle(.primary)
            }
        }
        .accessibilityValue(statusLabel)
        .swipeActions(edge: .trailing) {
            if !host.isQuick {
                Button("Supprimer", systemImage: "trash", role: .destructive) { confirmDelete = true }
                Button("Modifier", systemImage: "pencil") { model.editing = host }
            }
        }
        .swipeActions(edge: .leading) {
            if !host.isQuick {
                Button(host.favorite ? "Retirer des favoris" : "Favori", systemImage: host.favorite ? "star.slash" : "star") { toggleFavorite() }
                    .tint(.yellow)
            }
        }
        .contextMenu {
            Button("Se connecter", systemImage: "bolt.horizontal") { model.open(host) }
            Button("Se connecter dans tmux", systemImage: "rectangle.stack") { model.open(host, useTmux: true) }
            Button("Se connecter sans tmux", systemImage: "apple.terminal") { model.open(host, useTmux: false) }
            if !host.isQuick {
                Divider()
                Button("Modifier", systemImage: "pencil") { model.editing = host }
                Button("Dupliquer", systemImage: "plus.square.on.square") {
                    var copy = host
                    copy.id = UUID().uuidString.lowercased()
                    copy.name = "\(host.title) (copie)"
                    model.doc.hosts.append(copy)
                    model.saveHosts()
                }
                Button(host.favorite ? "Retirer des favoris" : "Ajouter aux favoris", systemImage: host.favorite ? "star.slash" : "star") { toggleFavorite() }
                Divider()
                Button("Supprimer", systemImage: "trash", role: .destructive) { confirmDelete = true }
            }
        }
        .confirmationDialog("Supprimer « \(host.title) » ?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Supprimer", role: .destructive) { model.delete(host) }
        } message: {
            Text("Son mot de passe enregistré sera aussi effacé.")
        }
    }

    private var statusLabel: String {
        switch model.status[host.id] {
        case "up": "En ligne"
        case "ping": "Répond au ping, pas en SSH"
        case "down": "Injoignable"
        default: ""
        }
    }

    private func toggleFavorite() {
        guard let i = model.doc.hosts.firstIndex(where: { $0.id == host.id }) else { return }
        model.doc.hosts[i].favorite.toggle()
        model.saveHosts()
    }
}

private struct SessionRow: View {
    @Environment(AppModel.self) private var model
    let session: TermSession

    var body: some View {
        Button { model.show(session) } label: {
            HStack(spacing: 12) {
                Image(systemName: "apple.terminal")
                    .font(.title3)
                    .foregroundStyle(.tint)
                    .frame(width: 30)
                VStack(alignment: .leading, spacing: 3) {
                    Text(session.host.title).font(.body.weight(.medium))
                    Text(status).font(.footnote).foregroundStyle(.secondary)
                }
                .foregroundStyle(.primary)
            }
        }
        .swipeActions {
            Button("Fermer", systemImage: "xmark", role: .destructive) { model.close(session) }
        }
    }

    private var status: String {
        switch session.phase {
        case .connecting: "Connexion…"
        case .open: "Connecté"
        case .closed(let err): err ?? "Déconnecté"
        }
    }
}

/// user@host:port without saving it first (or saving it on the way), like the desktop's quick connect.
struct QuickConnectView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var spec = ""
    @State private var save = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("utilisateur@exemple.fr:22", text: $spec)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .onSubmit(connect)
                } footer: {
                    Text("L'utilisateur et le port sont facultatifs.")
                }
                Section {
                    Toggle("Enregistrer dans mes hôtes", isOn: $save)
                }
                let recent = model.recentHosts.filter(\.isQuick)
                if !recent.isEmpty {
                    Section("Récentes") {
                        ForEach(recent) { h in Button(h.name) { spec = h.name } }
                    }
                }
            }
            .navigationTitle("Connexion rapide")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Annuler", role: .cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Se connecter", role: .confirm, action: connect)
                        .disabled(Host.quick(spec) == nil)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func connect() {
        guard var h = Host.quick(spec) else { return }
        dismiss()
        guard save else { return model.open(h) }
        h.id = UUID().uuidString.lowercased()
        h.icon = "server"
        model.doc.hosts.append(h)
        // Saved before connecting: the core looks the host up in hosts.json.
        let doc = model.doc
        Task {
            await model.run("save_hosts", ["doc": Core.object(doc)])
            model.open(h)
        }
    }
}
