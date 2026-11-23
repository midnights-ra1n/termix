import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var importing = false
    @State private var imported: Int?

    /// Monospaced fonts shipped with iOS ("" = SF Mono).
    private let fonts = [("", "SF Mono"), ("Menlo-Regular", "Menlo"), ("CourierNewPSMT", "Courier New")]

    var body: some View {
        @Bindable var model = model
        NavigationStack {
            Form {
                Section {
                    Picker("Thème", selection: Binding(get: { model.themeName }, set: { model.setTheme($0) })) {
                        ForEach(model.themeChoices, id: \.id) { t in
                            Label { Text(t.label) } icon: { Swatch(theme: Theme.resolve(t.id, user: model.userThemes, systemDark: model.systemDark)) }
                                .tag(t.id)
                        }
                    }
                    .pickerStyle(.navigationLink)
                } header: {
                    Text("Apparence")
                } footer: {
                    Text("Le thème est partagé avec vos autres appareils par la synchronisation.")
                }

                Section {
                    Picker("Police", selection: $model.fontName) {
                        ForEach(fonts, id: \.0) { Text($0.1).font(.custom($0.0, size: 15)).tag($0.0) }
                    }
                    Stepper(value: $model.fontSize, in: 7...32) {
                        LabeledContent("Taille", value: "\(Int(model.fontSize)) pt")
                    }
                    Toggle("Garder l'écran allumé", isOn: $model.keepAwake)
                } header: {
                    Text("Terminal")
                } footer: {
                    Text("Propres à cet appareil. Pincez le terminal pour changer la taille du texte ; l'écran reste allumé tant qu'une session est ouverte.")
                }

                Section {
                    Toggle("Sessions persistantes (tmux)", isOn: Binding(get: { model.tmuxDefault }, set: { model.setTmuxDefault($0) }))
                } footer: {
                    Text("Si tmux est installé sur le serveur, le shell tourne dedans : il continue quand iOS coupe la connexion et Termix le reprend au retour. Partagé avec vos ordinateurs ; chaque hôte peut aussi choisir.")
                }

                Section {
                    NavigationLink { KeysView() } label: {
                        LabeledContent("Clés SSH", value: model.keys.isEmpty ? "Aucune" : "\(model.keys.count)")
                    }
                    NavigationLink { SyncView() } label: {
                        LabeledContent("Synchronisation", value: syncSummary)
                    }
                }

                Section {
                    if let url = model.exportFile() {
                        ShareLink(item: url) { Label("Exporter les hôtes", systemImage: "square.and.arrow.up") }
                    }
                    Button("Importer des hôtes…", systemImage: "square.and.arrow.down") { importing = true }
                } header: {
                    Text("Hôtes")
                } footer: {
                    Text("Le fichier JSON de l'ordinateur (Fichier → Exporter les hôtes), sans les mots de passe.")
                }

                Section {
                    LabeledContent("Version", value: model.version)
                }
            }
            .navigationTitle("Réglages")
            .navigationBarTitleDisplayMode(.inline)
            .fileImporter(isPresented: $importing, allowedContentTypes: [.json]) { result in
                guard case .success(let url) = result else { return }
                do { imported = try model.importHosts(url) } catch { model.error = "Fichier d'hôtes illisible : \(error.localizedDescription)" }
            }
            .alert(imported.map { $0 == 0 ? "Aucun nouvel hôte" : "\($0) hôte(s) importé(s)" } ?? "",
                   isPresented: Binding(get: { imported != nil }, set: { if !$0 { imported = nil } })) {
                Button("OK", role: .cancel) {}
            }
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("OK", role: .confirm) { dismiss() }
                }
            }
        }
    }

    private var syncSummary: String {
        guard let s = model.sync, s.mode == "server", s.loggedIn else { return "Désactivée" }
        return s.unlocked ? "Activée" : "Verrouillée"
    }
}

/// The terminal background with its first ANSI colours, for the theme picker.
private struct Swatch: View {
    let theme: Theme

    var body: some View {
        RoundedRectangle(cornerRadius: 6)
            .fill(Color(theme.bg))
            .frame(width: 28, height: 20)
            .overlay {
                HStack(spacing: 2) {
                    ForEach(1..<5) { Circle().fill(Color(theme.ansi[$0])).frame(width: 4, height: 4) }
                }
            }
            .overlay { RoundedRectangle(cornerRadius: 6).strokeBorder(.separator) }
    }
}

struct SyncView: View {
    @Environment(AppModel.self) private var model
    @State private var url = ""
    @State private var user = ""
    @State private var password = ""
    @State private var passphrase = ""
    @State private var busy = false

    var body: some View {
        Form {
            if let s = model.sync, s.mode == "server", s.loggedIn {
                connected(s)
            } else {
                login
            }
        }
        .navigationTitle("Synchronisation")
        .disabled(busy)
        .overlay { if busy { ProgressView() } }
        .task {
            await model.refreshSync()
            if let s = model.sync, url.isEmpty {
                (url, user) = (s.serverUrl, s.user)
            }
        }
    }

    @ViewBuilder private var login: some View {
        Section {
            TextField("https://sync.exemple.fr", text: $url)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            TextField("Utilisateur", text: $user)
                .textContentType(.username)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("Mot de passe", text: $password)
                .textContentType(.password)
        } header: {
            Text("Serveur de synchronisation")
        } footer: {
            Text("Le compte de Réglages → Synchronisation sur l'ordinateur. Hôtes, mots de passe et clés sont chiffrés de bout en bout. La synchro en réseau local n'existe pas sur iOS.")
        }
        Section {
            Button("Se connecter") {
                act("sync_login", ["url": url.trimmingCharacters(in: .whitespaces), "user": user, "password": password])
            }
            .disabled(url.isEmpty || user.isEmpty || password.isEmpty)
        }
    }

    @ViewBuilder private func connected(_ s: SyncStatus) -> some View {
        Section {
            LabeledContent("Serveur", value: s.serverUrl)
            LabeledContent("Compte", value: s.user)
            LabeledContent("Dernière synchro") {
                if s.syncing {
                    ProgressView()
                } else if s.lastOk > 0 {
                    Text(Date(timeIntervalSince1970: TimeInterval(s.lastOk) / 1000), style: .relative)
                } else {
                    Text("Jamais")
                }
            }
            if !s.lastError.isEmpty {
                Label(s.lastError, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            }
            Button("Synchroniser maintenant") { act("sync_now") }
                .disabled(s.syncing)
        }

        if !s.unlocked {
            Section {
                SecureField("Phrase secrète", text: $passphrase)
                Button("Déverrouiller") { act("sync_unlock", ["passphrase": passphrase]) }
                    .disabled(passphrase.count < 8)
            } header: {
                Text("Coffre chiffré")
            } footer: {
                Text(s.vaultExists
                     ? "La phrase secrète choisie sur votre premier appareil : elle déchiffre mots de passe et clés."
                     : "Choisissez une phrase secrète (8 caractères minimum) : elle chiffre mots de passe et clés, le serveur ne la connaît pas.")
            }
        }

        if !s.conflicts.isEmpty {
            Section("Conflits") {
                ForEach(s.conflicts) { c in
                    VStack(alignment: .leading, spacing: 8) {
                        Text(label(c.key)).font(.body.weight(.medium))
                        HStack {
                            Button("Garder celui-ci") { act("sync_resolve", ["key": c.key, "keep": "mine"]) }
                            Spacer()
                            Button("Garder « \(c.theirs.device ?? "l'autre") »") { act("sync_resolve", ["key": c.key, "keep": "theirs"]) }
                        }
                        .buttonStyle(.borderless)
                    }
                }
            }
        }

        Section {
            if s.unlocked {
                Button("Verrouiller le coffre") { act("sync_lock") }
            }
            Button("Se déconnecter", role: .destructive) { act("sync_logout") }
        }
    }

    private func label(_ key: String) -> String {
        let id = key.split(separator: ":", maxSplits: 1).last.map(String.init) ?? key
        if key.hasPrefix("host:") { return "Hôte « \(model.doc.hosts.first { $0.id == id }?.title ?? id) »" }
        if key.hasPrefix("folder:") { return "Dossier « \(model.doc.folders.first { $0.id == id }?.name ?? id) »" }
        return key == "settings" ? "Réglages" : key
    }

    private func act(_ cmd: String, _ args: [String: Any] = [:]) {
        busy = true
        Task {
            await model.run(cmd, args)
            password = ""
            passphrase = ""
            await model.reload()
            busy = false
        }
    }
}
