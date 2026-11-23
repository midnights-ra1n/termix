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

    var body: some View {
        List {
            if let listing {
                ForEach(listing.entries) { e in
                    if e.dir {
                        NavigationLink(value: join(listing.path, e.name)) { EntryRow(entry: e) }
                            .swipeActions { remove(e) }
                    } else {
                        Button { Task { await download(e, in: listing.path) } } label: { EntryRow(entry: e) }
                            .foregroundStyle(.primary)
                            .swipeActions { remove(e) }
                    }
                }
            }
        }
        .overlay {
            if let error {
                ContentUnavailableView("Dossier illisible", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if listing == nil || busy {
                ProgressView()
            } else if listing?.entries.isEmpty == true {
                ContentUnavailableView("Dossier vide", systemImage: "folder")
            }
        }
        .navigationTitle(listing.map { ($0.path as NSString).lastPathComponent } ?? "Fichiers")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
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
        HStack(spacing: 12) {
            Image(systemName: entry.dir ? "folder.fill" : "doc")
                .foregroundStyle(entry.dir ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name).lineLimit(1)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var detail: String {
        let date = Date(timeIntervalSince1970: TimeInterval(entry.modified)).formatted(date: .abbreviated, time: .shortened)
        return entry.dir ? date : "\(ByteCountFormatter.string(fromByteCount: Int64(entry.size), countStyle: .file)) · \(date)"
    }
}
