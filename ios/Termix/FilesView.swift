import QuickLook
import SwiftUI

/// SFTP browser of a session, in the server panel: folders push like the Files app, files open in Quick Look
/// (saved to Termix's folder in Files first), uploads come from Files.
struct DirView: View {
    let session: TermSession
    let path: String
    @State private var listing: Listing?
    @State private var error: String?
    @State private var busy = false
    @State private var preview: URL?
    @State private var importing = false
    @State private var naming = false
    @State private var name = ""
    @State private var removing: Listing.Entry?
    @State private var query = ""
    @AppStorage("sftpSort") private var sort = "name"
    @AppStorage("sftpHidden") private var showHidden = true

    var body: some View {
        List {
            if let listing, !shown.isEmpty {
                Section {
                    ForEach(shown) { e in
                        let full = join(listing.path, e.name)
                        Group {
                            if e.dir {
                                NavigationLink(value: full) { EntryRow(entry: e) }
                            } else {
                                Button { Task { await download(e, in: listing.path) } } label: { EntryRow(entry: e) }
                                    .foregroundStyle(.primary)
                            }
                        }
                        .panelRow()
                        .swipeActions { remove(e) }
                        .contextMenu {
                            if !e.dir {
                                Button("Ouvrir", systemImage: "eye") { Task { await download(e, in: listing.path) } }
                            }
                            Button("Copier le chemin", systemImage: "doc.on.doc") { UIPasteboard.general.string = full }
                            remove(e)
                        }
                    }
                } header: {
                    Text(summary)
                }
            }
        }
        .themedBackground()
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .automatic), prompt: "Filtrer ce dossier")
        .overlay {
            if let error {
                ContentUnavailableView("Dossier illisible", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if listing == nil || busy {
                ProgressView()
            } else if listing?.entries.isEmpty == true {
                ContentUnavailableView("Dossier vide", systemImage: "folder")
            } else if shown.isEmpty {
                if query.isEmpty {
                    ContentUnavailableView("Que des fichiers cachés", systemImage: "eye.slash", description: Text("Menu ⋯ → Afficher les fichiers cachés."))
                } else {
                    ContentUnavailableView.search(text: query)
                }
            }
        }
        .navigationTitle(listing.map { ($0.path as NSString).lastPathComponent } ?? "Fichiers")
        .navigationSubtitle(listing?.path ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Menu {
                    Picker("Trier par", selection: $sort) {
                        Label("Nom", systemImage: "textformat").tag("name")
                        Label("Date", systemImage: "calendar").tag("date")
                        Label("Taille", systemImage: "arrow.up.and.down.text.horizontal").tag("size")
                    }
                    Toggle("Afficher les fichiers cachés", systemImage: "eye", isOn: $showHidden)
                } label: {
                    Label("Affichage", systemImage: "line.3.horizontal.decrease")
                }
                Menu {
                    Button("Envoyer des fichiers", systemImage: "square.and.arrow.up") { importing = true }
                    Button("Nouveau dossier", systemImage: "folder.badge.plus") { name = ""; naming = true }
                } label: {
                    Label("Ajouter", systemImage: "plus")
                }
                .disabled(listing == nil)
            }
        }
        .task { await load() }
        .refreshable { await load() }
        .quickLookPreview($preview)
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { Task { await upload(urls) } }
        }
        .alert("Nouveau dossier", isPresented: $naming) {
            TextField("Nom", text: $name)
            Button("Créer") { Task { await act("sftp_mkdir", ["path": join(listing?.path ?? path, name)]) } }
            Button("Annuler", role: .cancel) {}
        }
        .confirmationDialog("Supprimer « \(removing?.name ?? "") » ?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Supprimer", role: .destructive) {
                guard let e = removing, let base = listing?.path else { return }
                Task { await act("sftp_remove", ["path": join(base, e.name), "dir": e.dir]) }
            }
        } message: {
            Text(removing?.dir == true ? "Le dossier doit être vide." : "Cette action est définitive.")
        }
    }

    /// Folders first, then the chosen order; hidden files and the search filter applied.
    private var shown: [Listing.Entry] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return (listing?.entries ?? [])
            .filter { (showHidden || !$0.name.hasPrefix(".")) && (q.isEmpty || $0.name.localizedCaseInsensitiveContains(q)) }
            .sorted { a, b in
                if a.dir != b.dir { return a.dir }
                switch sort {
                case "date": return a.modified > b.modified
                case "size" where !a.dir: return a.size > b.size
                default: return a.name.localizedStandardCompare(b.name) == .orderedAscending
                }
            }
    }

    private var summary: String {
        let dirs = shown.filter(\.dir).count, files = shown.count - dirs
        return [dirs > 0 ? "\(dirs) dossier\(dirs > 1 ? "s" : "")" : nil, files > 0 ? "\(files) fichier\(files > 1 ? "s" : "")" : nil]
            .compactMap { $0 }.joined(separator: " · ")
    }

    private func remove(_ e: Listing.Entry) -> some View {
        Button("Supprimer", systemImage: "trash", role: .destructive) { removing = e }
    }

    private func join(_ dir: String, _ name: String) -> String {
        dir.hasSuffix("/") ? dir + name : dir + "/" + name
    }

    private func load() async {
        do {
            listing = try await Core.call("sftp_list", ["id": session.coreId, "path": listing?.path ?? path], as: Listing.self)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func act(_ cmd: String, _ args: [String: Any]) async {
        busy = true
        defer { busy = false }
        do {
            try await Core.call(cmd, args.merging(["id": session.coreId]) { a, _ in a })
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Into Documents: Termix's folder in the Files app ("On my iPhone › Termix").
    private func download(_ e: Listing.Entry, in dir: String) async {
        let docs = URL.documentsDirectory
        let local = docs.appending(path: e.name)
        busy = true
        defer { busy = false }
        do {
            try await Core.call("sftp_download", ["id": session.coreId, "remote": join(dir, e.name), "local": local.path])
            preview = local
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func upload(_ urls: [URL]) async {
        guard let base = listing?.path else { return }
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            await act("sftp_upload", ["local": url.path, "remote": join(base, url.lastPathComponent)])
            if scoped { url.stopAccessingSecurityScopedResource() }
        }
    }
}

private struct EntryRow: View {
    let entry: Listing.Entry

    var body: some View {
        let (symbol, color) = kind
        HStack(spacing: 12) {
            IconTile(systemImage: symbol, color: color)
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name).font(.body.weight(entry.dir ? .semibold : .regular)).lineLimit(1).truncationMode(.middle)
                    .foregroundStyle(entry.name.hasPrefix(".") ? .secondary : .primary)
                Text(detail).font(.caption).foregroundStyle(.secondary).monospacedDigit()
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(entry.dir ? "Dossier \(entry.name)" : entry.name)
    }

    private var detail: String {
        let d = Date(timeIntervalSince1970: TimeInterval(entry.modified))
        // Recent changes read better as "il y a 3 heures".
        let date = d.timeIntervalSinceNow > -7 * 86400 ? d.formatted(.relative(presentation: .named)) : d.formatted(date: .abbreviated, time: .omitted)
        return entry.dir ? date : "\(ByteCountFormatter.string(fromByteCount: Int64(entry.size), countStyle: .file)) · \(date)"
    }

    /// Icon and colour by file type (nil colour: the theme's tint, for folders).
    private var kind: (String, Color?) {
        if entry.dir { return ("folder.fill", nil) }
        let n = entry.name.lowercased(), ext = (n as NSString).pathExtension
        if n.hasPrefix("id_") || ["pem", "key", "crt", "cer", "pub", "p12"].contains(ext) { return ("key.fill", .yellow) }
        if n == "dockerfile" || n.hasPrefix("docker-compose") || n.hasPrefix("compose.") { return ("shippingbox.fill", .cyan) }
        switch ext {
        case "png", "jpg", "jpeg", "gif", "webp", "heic", "svg", "avif", "bmp": return ("photo.fill", .purple)
        case "mp4", "mov", "mkv", "avi", "webm": return ("film.fill", .pink)
        case "mp3", "flac", "wav", "ogg", "m4a": return ("music.note", .pink)
        case "zip", "gz", "tgz", "xz", "bz2", "zst", "tar", "7z", "rar", "deb", "rpm": return ("doc.zipper", .brown)
        case "sh", "bash", "zsh", "fish", "py", "js", "ts", "rs", "go", "c", "h", "cpp", "swift", "rb", "php", "java", "lua", "pl":
            return ("chevron.left.forwardslash.chevron.right", .blue)
        case "conf", "cfg", "ini", "toml", "yaml", "yml", "json", "xml", "env", "service", "timer": return ("gearshape.fill", .teal)
        case "log": return ("list.bullet.rectangle", .orange)
        case "pdf": return ("doc.richtext.fill", .red)
        case "md", "txt", "rst", "csv": return ("doc.text.fill", .secondary)
        default: return ("doc.fill", .secondary)
        }
    }
}
