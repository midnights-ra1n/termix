import Foundation

/// hosts.json, as written by config.rs (every field is always present).
struct Host: Codable, Identifiable, Hashable {
    var id = UUID().uuidString.lowercased()
    var name = ""
    var host = ""
    var port = 22
    var user = ""
    var keyPath = ""
    var keyId = ""
    var description = ""
    var icon = "server"
    var tags: [String] = []
    var folder: String?
    var favorite = false
    var tmux = ""
    var os = ""

    var title: String { name.isEmpty ? host : name }
    var address: String {
        (user.isEmpty ? "" : "\(user)@") + host + (port == 22 || port == 0 ? "" : ":\(port)")
    }

    /// The desktop's host icons (HOST_ICONS in ui.js) as SF Symbols.
    static let icons: [(id: String, symbol: String)] = [
        ("server", "server.rack"), ("cloud", "cloud"), ("database", "cylinder.split.1x2"), ("desktop", "desktopcomputer"),
        ("board", "cpu"), ("docker", "shippingbox"), ("router", "wifi.router"), ("firewall", "shield.lefthalf.filled"),
        ("web", "globe"), ("terminal", "apple.terminal"), ("home", "house"), ("box", "cube"),
        ("nas", "externaldrive"), ("prod", "flame"), ("deploy", "paperplane"),
    ]
    var symbol: String { Self.icons.first { $0.id == icon }?.symbol ?? "server.rack" }

    var isQuick: Bool { id.hasPrefix("quick:") }

    /// An unsaved host from "user@host:port" (user and port optional): the same rules and id as quickHost() in panes.js.
    static func quick(_ spec: String) -> Host? {
        let pattern = #/^(?:([^@\s]+)@)?([^\s@:]+|\[[^\]\s]+\])(?::(\d{1,5}))?$/#
        guard let m = spec.trimmingCharacters(in: .whitespaces).wholeMatch(of: pattern) else { return nil }
        let port = m.3.flatMap { Int($0) } ?? 22
        guard (1...65535).contains(port) else { return nil }
        let user = m.1.map(String.init) ?? "", target = String(m.2)
        let name = (user.isEmpty ? "" : user + "@") + target + (port == 22 ? "" : ":\(port)")
        var h = Host(name: name, host: target.trimmingCharacters(in: CharacterSet(charactersIn: "[]")), port: port, user: user, icon: "terminal")
        h.id = "quick:" + name
        return h
    }
}

struct Folder: Codable, Identifiable, Hashable {
    var id = UUID().uuidString.lowercased()
    var name = ""
    var parent: String?
}

struct HostsDoc: Codable {
    var folders: [Folder] = []
    var hosts: [Host] = []

    /// "Prod / Web" for pickers.
    func path(_ id: String?) -> String {
        var names: [String] = [], cur = id, seen = Set<String>()
        while let c = cur, !seen.contains(c), let f = folders.first(where: { $0.id == c }) {
            names.insert(f.name, at: 0)
            seen.insert(c)
            cur = f.parent
        }
        return names.joined(separator: " / ")
    }
}

/// ssh::Stats: raw counters, CPU % and network rates come from two samples.
struct Stats: Decodable {
    var cpuTotal: UInt64, cpuIdle: UInt64
    var memTotalKb: UInt64, memAvailKb: UInt64
    var diskTotalKb: UInt64, diskUsedKb: UInt64
    var load: [Double]
    var netRx: UInt64, netTx: UInt64
    var hostname: String, osName: String, kernel: String
    var uptimeSecs: UInt64
}

struct Listing: Decodable {
    struct Entry: Decodable, Identifiable {
        var name: String, dir: Bool, size: UInt64, modified: UInt64
        var id: String { name }
    }
    var path: String
    var entries: [Entry]
}

/// syncd::status().
struct SyncStatus: Decodable {
    struct Conflict: Decodable, Identifiable {
        struct Side: Decodable { var ts: Int64; var device: String? }
        var key: String
        var theirs: Side
        var id: String { key }
    }
    var mode: String
    var serverUrl: String
    var user: String
    var loggedIn: Bool
    var unlocked: Bool
    var vaultExists: Bool
    var lastOk: Int64
    var lastError: String
    var syncing: Bool
    var conflicts: [Conflict]
}

/// A private key in the keychain: synced through the vault, or this device's only.
struct SSHKey: Decodable, Identifiable {
    var id: String
    var name: String
    var pub: String?
    var synced: Bool

    /// "ed25519", "rsa"… (empty for a passphrase-protected key, unreadable until a connection).
    var kind: String {
        (pub ?? "").split(separator: " ").first.map { $0.replacingOccurrences(of: "ssh-", with: "") } ?? ""
    }
}

/// ssh::TmuxSession.
struct TmuxSession: Decodable, Identifiable {
    var name: String
    var windows: Int
    var attached: Bool
    var activity: Int64
    var command: String
    var id: String { name }
}

/// ssh::Docker: read-only container list.
struct Docker: Decodable {
    struct Container: Decodable, Identifiable {
        var name: String, image: String, state: String, status: String
        var id: String { name }
    }
    var installed: Bool
    var containers: [Container]
    var error: String
}

/// Another device's open tabs (syncd::sync_workspaces), flattened to their panes.
struct Workspace: Decodable, Identifiable {
    struct Pane: Hashable { var host: String; var tmux: String? }
    var name: String
    var ts: Int64
    var panes: [Pane]
    var id: String { "\(name)-\(ts)" }

    /// A tab: a pane {host, tmux} or a split {a, b}.
    private final class Node: Decodable {
        let host: String?, tmux: String?, a: Node?, b: Node?
        var leaves: [Pane] { host.map { [Pane(host: $0, tmux: tmux)] } ?? (a?.leaves ?? []) + (b?.leaves ?? []) }
    }
    private enum Keys: String, CodingKey { case name, ts, tabs }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        ts = try c.decodeIfPresent(Int64.self, forKey: .ts) ?? 0
        panes = try c.decode([Node].self, forKey: .tabs).flatMap(\.leaves)
    }
}
