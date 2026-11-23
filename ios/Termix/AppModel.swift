import SwiftUI
import UIKit

@MainActor @Observable
final class AppModel {
    var doc = HostsDoc()
    var version = ""
    var themeName = "termix"
    @ObservationIgnored var userThemes: [String: [String: Any]] = [:]
    var systemDark = true
    var sessions: [TermSession] = []
    var current: TermSession?
    var column = NavigationSplitViewColumn.sidebar
    var sync: SyncStatus?
    var keys: [SSHKey] = []
    var error: String?
    /// Host editor and folder name prompts, opened from anywhere in the host tree.
    var editing: Host?
    var naming: Folder?
    var quickConnect = false
    /// The server panel (stats, files, Docker, tmux): a column on iPad, a sheet on iPhone.
    var panel = false
    /// Host id → "up" (SSH answers), "ping" or "down" (probe.rs).
    var status: [String: String] = [:]
    /// Latest connections, newest first: host ids and "quick:user@host:port".
    var recent: [String] = []
    /// Other devices' open tabs, to carry on here.
    var workspaces: [Workspace] = []
    /// Settings → Terminal → tmux, shared with the computers.
    var tmuxDefault = false

    // Terminal font: per device (the desktop's synced fonts aren't installed on iOS).
    var fontName = UserDefaults.standard.string(forKey: "fontName") ?? "" {
        didSet { UserDefaults.standard.set(fontName, forKey: "fontName"); restyle() }
    }
    var fontSize = UserDefaults.standard.object(forKey: "fontSize") as? Double ?? 13 {
        didSet { UserDefaults.standard.set(fontSize, forKey: "fontSize"); restyle() }
    }
    /// No auto-lock while a session is open.
    var keepAwake = UserDefaults.standard.bool(forKey: "keepAwake") {
        didSet { UserDefaults.standard.set(keepAwake, forKey: "keepAwake"); updateIdleTimer() }
    }

    @ObservationIgnored private var inBackground = false
    @ObservationIgnored private var backgroundTask = UIBackgroundTaskIdentifier.invalid
    @ObservationIgnored private var pinchBase = 13.0

    var theme: Theme { Theme.resolve(themeName, user: userThemes, systemDark: systemDark) }
    var font: UIFont {
        (fontName.isEmpty ? nil : UIFont(name: fontName, size: fontSize)) ?? .monospacedSystemFont(ofSize: fontSize, weight: .regular)
    }
    /// Built-in themes in the desktop's order, then the user's own.
    var themeChoices: [(id: String, label: String)] {
        let builtIn = Theme.list.compactMap { t in (t["id"] as? String).map { ($0, t["label"] as? String ?? $0) } }
        let own = userThemes.keys.sorted().filter { Theme.builtIn[$0] == nil }.map { ($0, userThemes[$0]?["label"] as? String ?? $0) }
        return builtIn + own
    }

    /// Saved hosts and quick connects of the latest sessions.
    var recentHosts: [Host] {
        recent.compactMap { id in id.hasPrefix("quick:") ? Host.quick(String(id.dropFirst(6))) : doc.hosts.first { $0.id == id } }
    }

    init() {
        Core.onEvent = { [weak self] kind, id, data in self?.event(kind, id, data) }
        Core.start(device: UIDevice.current.model)
        Task { await reload() }
    }

    private func event(_ kind: String, _ id: String, _ data: Data) {
        switch kind {
        case "data": sessions.first { $0.coreId == id }?.feed(data)
        case "ssh-closed":
            guard let s = sessions.first(where: { $0.coreId == id }) else { return }
            s.closed(ended: String(decoding: data, as: UTF8.self) == "true")
            // In the background it waits for the app to come back (see scenePhase).
            if !inBackground { s.reconnectIfDropped() }
        case "sync-status":
            sync = try? Core.decoder.decode(SyncStatus.self, from: data)
            Task { await refreshWorkspaces() }
        case "sync-changed": Task { await reload() }
        default: break
        }
    }

    func reload() async {
        do {
            let boot = try JSONSerialization.jsonObject(with: try await Core.call("boot")) as? [String: Any] ?? [:]
            let settings = boot["settings"] as? [String: Any] ?? [:]
            version = boot["version"] as? String ?? ""
            userThemes = settings["themes"] as? [String: [String: Any]] ?? [:]
            themeName = (settings["appearance"] as? [String: Any])?["theme"] as? String ?? "termix"
            tmuxDefault = (settings["terminal"] as? [String: Any])?["tmux"] as? Bool ?? false
            doc = try await Core.call("get_hosts", as: HostsDoc.self)
            restyle()
        } catch {
            self.error = error.localizedDescription
        }
        await reloadKeys()
        await refreshSync()
        await refreshRecent()
        await refreshWorkspaces()
    }

    func reloadKeys() async {
        keys = (try? await Core.call("keys", as: [SSHKey].self)) ?? []
    }

    func refreshSync() async {
        sync = try? await Core.call("sync_status", as: SyncStatus.self)
    }

    func refreshRecent() async {
        recent = (try? await Core.call("recent", as: [String].self)) ?? []
    }

    func refreshWorkspaces() async {
        workspaces = (try? await Core.call("sync_workspaces", as: [Workspace].self)) ?? []
    }

    func probe() async {
        if let s = try? await Core.call("probe", as: [String: String].self) { status = s }
    }

    /// Runs a core call, showing its error.
    func run(_ cmd: String, _ args: [String: Any] = [:]) async {
        do { try await Core.call(cmd, args) } catch { self.error = error.localizedDescription }
    }

    func saveHosts() {
        let doc = doc
        Task { await run("save_hosts", ["doc": Core.object(doc)]) }
    }

    func setSecret(_ id: String, _ value: String) {
        Task { await run("set_secret", ["id": id, "value": value]) }
    }

    func setTheme(_ name: String) {
        themeName = name
        restyle()
        Task { await run("set_setting", ["path": "/appearance/theme", "value": name]) }
    }

    func setTmuxDefault(_ on: Bool) {
        tmuxDefault = on
        Task { await run("set_setting", ["path": "/terminal/tmux", "value": on]) }
    }

    func restyle() {
        let (t, f) = (theme, font)
        for s in sessions {
            s.apply(t)
            if s.view.font != f { s.view.font = f }
        }
    }

    func zoom(_ step: Double) {
        fontSize = min(max(fontSize + step, 7), 32)
    }

    /// Pinch: relative to the size when the fingers landed, by whole points.
    func pinch(_ scale: CGFloat, began: Bool) {
        if began { pinchBase = fontSize }
        let size = min(max((pinchBase * scale).rounded(), 7), 32)
        if size != fontSize { fontSize = size }
    }

    /// `resume`: a tmux session to attach. Otherwise tmux follows `useTmux` (one connection), then the host, then
    /// the general setting, like wantsTmux() on the desktop.
    func open(_ host: Host, useTmux: Bool? = nil, resume: String? = nil) {
        if let resume, let s = sessions.first(where: { $0.host.id == host.id && $0.tmux == resume }) { return show(s) }
        let want = useTmux ?? (host.tmux == "on" || (host.tmux != "off" && tmuxDefault))
        let tmux = resume ?? (want ? "tx-" + UUID().uuidString.prefix(8).lowercased() : nil)
        let s = TermSession(host: host, tmux: tmux, theme: theme, font: font)
        s.model = self
        sessions.append(s)
        show(s)
        sessionsChanged()
        Task {
            await s.connect()
            await refreshRecent()
        }
    }

    /// Another device's tabs: every pane of a saved host, its tmux session attached again.
    func open(_ ws: Workspace) {
        for p in ws.panes {
            guard let h = doc.hosts.first(where: { $0.id == p.host }) else { continue }
            open(h, useTmux: p.tmux == nil ? nil : true, resume: p.tmux)
        }
    }

    func show(_ s: TermSession) {
        current = s
        column = .detail
    }

    /// Next or previous session (⌘] / ⌘[).
    func cycle(_ step: Int) {
        guard !sessions.isEmpty else { return }
        let i = sessions.firstIndex { $0 === current } ?? 0
        show(sessions[(i + step + sessions.count) % sessions.count])
    }

    func close(_ s: TermSession) {
        s.close()
        sessions.removeAll { $0 === s }
        if current === s {
            current = sessions.last
            if current == nil { column = .sidebar }
        }
        sessionsChanged()
    }

    private func sessionsChanged() {
        updateIdleTimer()
        // This device's sessions, so a computer can carry on with them (tmux sessions are attached there).
        let tabs = sessions.filter { !$0.host.isQuick }.map { ["host": $0.host.id, "tmux": $0.tmux ?? NSNull()] as [String: Any] }
        Task { try? await Core.call("sync_set_workspace", ["tabs": tabs]) }
    }

    private func updateIdleTimer() {
        UIApplication.shared.isIdleTimerDisabled = keepAwake && !sessions.isEmpty
    }

    /// iOS suspends the app soon after it leaves the screen. Asking for the usual extra half-minute keeps the
    /// connections through a quick detour (copying a code, a message); lost ones reconnect when the app is back.
    func scenePhase(_ phase: ScenePhase) {
        switch phase {
        case .background:
            inBackground = true
            if backgroundTask == .invalid, !sessions.isEmpty {
                backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "ssh") { [weak self] in self?.endBackgroundTask() }
            }
        case .active:
            inBackground = false
            endBackgroundTask()
            for s in sessions {
                s.reconnectIfDropped()
                s.checkAlive()
            }
            Task { await refreshWorkspaces() }
        default:
            break
        }
    }

    private func endBackgroundTask() {
        guard backgroundTask != .invalid else { return }
        UIApplication.shared.endBackgroundTask(backgroundTask)
        backgroundTask = .invalid
    }

    func delete(_ host: Host) {
        doc.hosts.removeAll { $0.id == host.id }
        saveHosts()
        setSecret(host.id, "")
        setSecret(host.id + ":passphrase", "")
    }

    /// Like the desktop: hosts and subfolders move up to the parent.
    func delete(_ folder: Folder) {
        for i in doc.hosts.indices where doc.hosts[i].folder == folder.id { doc.hosts[i].folder = folder.parent }
        for i in doc.folders.indices where doc.folders[i].parent == folder.id { doc.folders[i].parent = folder.parent }
        doc.folders.removeAll { $0.id == folder.id }
        saveHosts()
    }

    /// The desktop's JSON export (hosts and folders, no secrets).
    func exportFile() -> URL? {
        let url = FileManager.default.temporaryDirectory.appending(path: "termix-hosts.json")
        guard let data = try? Core.encoder.encode(doc), (try? data.write(to: url)) != nil else { return nil }
        return url
    }

    /// Adds the hosts and folders of an export that aren't here yet. Returns how many hosts were added.
    func importHosts(_ url: URL) throws -> Int {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let other = try Core.decoder.decode(HostsDoc.self, from: Data(contentsOf: url))
        let folders = Set(doc.folders.map(\.id)), hosts = Set(doc.hosts.map(\.id))
        doc.folders += other.folders.filter { !folders.contains($0.id) }
        let added = other.hosts.filter { !hosts.contains($0.id) }
        doc.hosts += added
        saveHosts()
        return added.count
    }
}
