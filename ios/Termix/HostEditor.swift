import SwiftUI

struct HostEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var host: Host
    @State private var tags: String
    @State private var password = ""
    @State private var passphrase = ""
    @State private var hasPassword = false
    @State private var addingKey = false
    private let isNew: Bool

    init(host: Host) {
        _host = State(initialValue: host)
        _tags = State(initialValue: host.tags.joined(separator: ", "))
        isNew = host.host.isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Nom", text: $host.name)
                    TextField("Hôte ou adresse IP", text: $host.host)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    TextField("Utilisateur", text: $host.user)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    LabeledContent("Port") {
                        TextField("22", value: $host.port, format: .number.grouping(.never))
                            .keyboardType(.numberPad)
                            .multilineTextAlignment(.trailing)
                    }
                }

                Section {
                    SecureField(hasPassword ? "Mot de passe enregistré" : "Mot de passe", text: $password)
                        .textContentType(.password)
                    if hasPassword {
                        Button("Oublier le mot de passe", role: .destructive) {
                            model.setSecret(host.id, "")
                            hasPassword = false
                        }
                    }
                    Picker("Clé SSH", selection: $host.keyId) {
                        Text("Aucune").tag("")
                        ForEach(model.keys) { k in
                            Label(k.name, systemImage: k.synced ? "arrow.triangle.2.circlepath" : "iphone").tag(k.id)
                        }
                        if !host.keyId.isEmpty && !model.keys.contains(where: { $0.id == host.keyId }) {
                            Text("Clé absente de cet appareil").tag(host.keyId)
                        }
                    }
                    Button("Ajouter une clé…", systemImage: "key") { addingKey = true }
                    if !host.keyId.isEmpty {
                        SecureField("Phrase de passe de la clé", text: $passphrase)
                    }
                } header: {
                    Text("Authentification")
                } footer: {
                    if !host.keyPath.isEmpty {
                        Text("Fichier de clé de l'ordinateur (\(host.keyPath)) : absent de cet appareil, la clé choisie ici ou le mot de passe sont utilisés.")
                    } else {
                        Text("Les clés de Réglages › Clés SSH, dont celles de la synchro une fois le coffre déverrouillé.")
                    }
                }

                Section {
                    Picker("Sessions persistantes (tmux)", selection: $host.tmux) {
                        Text(model.tmuxDefault ? "Réglage général (activé)" : "Réglage général (désactivé)").tag("")
                        Text("Toujours").tag("on")
                        Text("Jamais").tag("off")
                    }
                } footer: {
                    Text("Avec tmux sur le serveur, le shell continue quand la connexion est perdue et Termix le reprend. Un appui long sur l'hôte choisit pour une seule connexion.")
                }

                Section("Organisation") {
                    Picker("Dossier", selection: $host.folder) {
                        Text("Aucun").tag(String?.none)
                        ForEach(model.doc.folders.map { ($0.id, model.doc.path($0.id)) }.sorted { $0.1 < $1.1 }, id: \.0) { id, path in
                            Text(path).tag(Optional(id))
                        }
                    }
                    TextField("Tags, séparés par des virgules", text: $tags)
                        .textInputAutocapitalization(.never)
                    Toggle("Favori", isOn: $host.favorite)
                }

                Section("Icône") {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 44))], spacing: 8) {
                        ForEach(Host.icons, id: \.id) { icon in
                            let on = host.icon == icon.id
                            Button { host.icon = icon.id } label: {
                                Image(systemName: icon.symbol)
                                    .font(.title3)
                                    .frame(width: 44, height: 44)
                                    .background(on ? AnyShapeStyle(.tint.opacity(0.2)) : AnyShapeStyle(.clear), in: .rect(cornerRadius: 10))
                            }
                            .buttonStyle(.plain)
                            .foregroundStyle(on ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                            .accessibilityLabel(icon.id)
                            .accessibilityAddTraits(on ? .isSelected : [])
                        }
                    }
                    .padding(.vertical, 4)
                }

                Section("Description") {
                    TextField("Notes", text: $host.description, axis: .vertical)
                        .lineLimit(3...8)
                }
            }
            .navigationTitle(isNew ? "Nouvel hôte" : host.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Annuler", role: .cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Enregistrer", role: .confirm) { save() }
                        .disabled(host.host.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .sheet(isPresented: $addingKey) { KeyImportView { host.keyId = $0 } }
            .task {
                hasPassword = (try? await Core.call("has_secret", ["id": host.id], as: Bool.self)) ?? false
            }
        }
    }

    private func save() {
        host.host = host.host.trimmingCharacters(in: .whitespaces)
        host.tags = tags.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        if host.port <= 0 || host.port > 65535 { host.port = 22 }
        if let i = model.doc.hosts.firstIndex(where: { $0.id == host.id }) {
            model.doc.hosts[i] = host
        } else {
            model.doc.hosts.append(host)
        }
        model.saveHosts()
        if !password.isEmpty { model.setSecret(host.id, password) }
        if !passphrase.isEmpty { model.setSecret(host.id + ":passphrase", passphrase) }
        dismiss()
    }
}
