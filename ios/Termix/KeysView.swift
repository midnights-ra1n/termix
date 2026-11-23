import SwiftUI
import UIKit

/// Settings → SSH keys: every key this device can use. Each one is kept on this device only or synced through the
/// encrypted vault; move one with a swipe, or all of them from the ⋯ menu.
struct KeysView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false
    @State private var deleting: SSHKey?
    /// Keys about to move, and where: confirmed first, since "this device only" removes them from the others.
    @State private var moving: (keys: [SSHKey], synced: Bool)?

    private var canSync: Bool { model.sync?.unlocked == true }
    private var synced: [SSHKey] { model.keys.filter(\.synced) }
    private var local: [SSHKey] { model.keys.filter { !$0.synced } }

    var body: some View {
        List {
            ThemedRows {
                if !synced.isEmpty {
                    Section {
                        ForEach(synced) { row($0) }
                    } header: {
                        Text("Synchronisées")
                    } footer: {
                        Text("Chiffrées dans le coffre, sur tous vos appareils.")
                    }
                }
                if !local.isEmpty {
                    Section {
                        ForEach(local) { row($0) }
                    } header: {
                        Text("Sur cet appareil uniquement")
                    } footer: {
                        Text(canSync ? "Ne quittent pas cet appareil. Balayez vers la droite pour en synchroniser une."
                                     : "Pour les synchroniser, connectez la synchro et déverrouillez le coffre (Réglages → Synchronisation).")
                    }
                }
            }
        }
        .themedBackground()
        .overlay {
            if model.keys.isEmpty {
                ContentUnavailableView {
                    Label("Aucune clé", systemImage: "key")
                } description: {
                    Text("Collez une clé privée ou choisissez son fichier.")
                } actions: {
                    Button("Ajouter une clé") { adding = true }
                }
            }
        }
        .navigationTitle("Clés SSH")
        .toolbar {
            Button("Ajouter une clé", systemImage: "plus") { adding = true }
            Menu {
                Button("Tout synchroniser (\(local.count))", systemImage: "arrow.triangle.2.circlepath") { moving = (local, true) }
                    .disabled(!canSync || local.isEmpty)
                Button("Tout garder sur cet appareil (\(synced.count))", systemImage: "iphone") { moving = (synced, false) }
                    .disabled(synced.isEmpty)
            } label: {
                Label("Toutes les clés", systemImage: "ellipsis")
            }
            .disabled(model.keys.isEmpty)
        }
        .sheet(isPresented: $adding) { KeyImportView() }
        .confirmationDialog("Supprimer « \(deleting?.name ?? "") » ?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible) {
            Button("Supprimer", role: .destructive) {
                guard let k = deleting else { return }
                Task {
                    await model.run("delete_key", ["id": k.id])
                    await model.reloadKeys()
                }
            }
        } message: {
            Text(deleting?.synced == true
                 ? "Elle disparaîtra de tous vos appareils synchronisés."
                 : "Les hôtes qui l'utilisent passeront au mot de passe.")
        }
        .confirmationDialog(moving.map { $0.synced ? "Synchroniser \($0.keys.count) clé(s) ?" : "Garder \($0.keys.count) clé(s) sur cet appareil ?" } ?? "",
                            isPresented: Binding(get: { moving != nil }, set: { if !$0 { moving = nil } }), titleVisibility: .visible) {
            Button(moving?.synced == true ? "Synchroniser" : "Garder sur cet appareil", role: moving?.synced == true ? nil : .destructive) {
                guard let m = moving else { return }
                Task {
                    await model.run("keys_set_synced", ["ids": m.keys.map(\.id), "synced": m.synced])
                    await model.reloadKeys()
                }
            }
        } message: {
            Text(moving?.synced == true
                 ? "Chiffrées dans le coffre, elles seront disponibles sur vos autres appareils."
                 : "Elles seront retirées du serveur et de vos autres appareils.")
        }
        .task { await model.reloadKeys() }
        .refreshable { await model.reloadKeys() }
    }

    private func row(_ k: SSHKey) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(k.name).font(.body.weight(.medium))
            Text(k.kind.isEmpty ? "chiffrée" : k.kind).font(.caption.monospaced()).foregroundStyle(.secondary)
        }
        .contextMenu { actions(k) }
        .swipeActions(edge: .trailing) {
            Button("Supprimer", systemImage: "trash", role: .destructive) { deleting = k }
        }
        .swipeActions(edge: .leading) { move(k) }
    }

    @ViewBuilder private func move(_ k: SSHKey) -> some View {
        if k.synced {
            Button("Garder ici", systemImage: "iphone") { moving = ([k], false) }.tint(.orange)
        } else if canSync {
            Button("Synchroniser", systemImage: "arrow.triangle.2.circlepath") { moving = ([k], true) }.tint(.accentColor)
        }
    }

    @ViewBuilder private func actions(_ k: SSHKey) -> some View {
        if let pub = k.pub, !pub.isEmpty {
            Button("Copier la clé publique", systemImage: "doc.on.doc") { UIPasteboard.general.string = pub }
        }
        if k.synced {
            Button("Garder sur cet appareil uniquement", systemImage: "iphone") { moving = ([k], false) }
        } else {
            Button("Synchroniser avec mes appareils", systemImage: "arrow.triangle.2.circlepath") { moving = ([k], true) }
                .disabled(!canSync)
        }
        Divider()
        Button("Supprimer", systemImage: "trash", role: .destructive) { deleting = k }
    }
}

/// Adds a private key from the clipboard or a file. `onImport` gets the new key's id.
struct KeyImportView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var onImport: (String) -> Void = { _ in }
    @State private var text = ""
    @State private var name = ""
    @State private var sync = true
    @State private var picking = false
    @State private var busy = false

    private var canSync: Bool { model.sync?.unlocked == true }

    var body: some View {
        NavigationStack {
            Form {
                ThemedRows {
                    Section {
                        // PasteButton reads the clipboard without iOS's permission prompt.
                        PasteButton(payloadType: String.self) { strings in
                            if let s = strings.first { text = s }
                        }
                        Button("Choisir un fichier…", systemImage: "doc") { picking = true }
                    } footer: {
                        Text("Depuis le Mac : pbcopy < ~/.ssh/id_ed25519, puis Coller ici grâce au presse-papiers universel.")
                    }

                    Section("Clé privée") {
                        TextEditor(text: $text)
                            .font(.caption.monospaced())
                            .frame(minHeight: 140)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .accessibilityLabel("Clé privée")
                    }

                    Section {
                        TextField("Nom (facultatif)", text: $name)
                        if canSync {
                            Toggle("Synchroniser avec mes appareils", isOn: $sync)
                        }
                    } footer: {
                        Text(canSync && sync
                             ? "Chiffrée dans le coffre de synchro, puis disponible sur vos autres appareils."
                             : "Gardée dans le trousseau de cet appareil. Une clé protégée par une phrase de passe la demande dans la fiche de l'hôte.")
                    }
                }
            }
            .themedBackground()
            .navigationTitle("Ajouter une clé")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Annuler", role: .cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Ajouter", role: .confirm) { Task { await add() } }
                        .disabled(busy || (!text.contains("PRIVATE KEY") && !text.contains("PuTTY-User-Key-File")))
                }
            }
            .disabled(busy)
            .fileImporter(isPresented: $picking, allowedContentTypes: [.item]) { result in
                guard case .success(let url) = result else { return }
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                do {
                    text = try String(contentsOf: url, encoding: .utf8)
                    if name.isEmpty { name = url.lastPathComponent }
                } catch {
                    model.error = "Fichier illisible : ce n'est pas une clé privée au format texte."
                }
            }
        }
    }

    private func add() async {
        busy = true
        defer { busy = false }
        do {
            let id = try await Core.call("import_key", ["pem": text, "name": name, "sync": canSync && sync], as: String.self)
            await model.reloadKeys()
            onImport(id)
            dismiss()
        } catch {
            model.error = error.localizedDescription
        }
    }
}
