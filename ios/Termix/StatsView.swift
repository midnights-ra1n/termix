import SwiftUI

/// Server stats (Linux /proc, like the desktop's right panel), refreshed every 2 s while shown.
struct StatsView: View {
    let session: TermSession
    @State private var cur: Stats?
    @State private var prev: Stats?
    @State private var error: String?
    private let every = 2.0

    var body: some View {
        Form {
            if let s = cur {
                Section(s.hostname) {
                    LabeledContent("Système", value: s.osName)
                    LabeledContent("Noyau", value: s.kernel)
                    LabeledContent("Allumé depuis", value: Duration.seconds(s.uptimeSecs).formatted(.units(allowed: [.days, .hours, .minutes], width: .abbreviated, maximumUnitCount: 2)))
                }
                Section("Utilisation") {
                    meter("Processeur", cpu, cpu.map { $0.formatted(.percent.precision(.fractionLength(0))) } ?? "…")
                    meter("Mémoire", ratio(s.memTotalKb - min(s.memAvailKb, s.memTotalKb), s.memTotalKb),
                          "\(bytes(s.memTotalKb - min(s.memAvailKb, s.memTotalKb))) sur \(bytes(s.memTotalKb))")
                    meter("Disque /", ratio(s.diskUsedKb, s.diskTotalKb), "\(bytes(s.diskUsedKb)) sur \(bytes(s.diskTotalKb))")
                }
                Section("Charge et réseau") {
                    LabeledContent("Charge (1, 5, 15 min)", value: s.load.map { $0.formatted(.number.precision(.fractionLength(2))) }.joined(separator: "  "))
                    LabeledContent("Réception", value: rate(\.netRx))
                    LabeledContent("Envoi", value: rate(\.netTx))
                }
            } else if let error {
                ContentUnavailableView("Statistiques indisponibles", systemImage: "exclamationmark.triangle", description: Text(error))
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        .monospacedDigit()
        .navigationTitle("Statistiques")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            while !Task.isCancelled {
                do {
                    let s = try await Core.call("ssh_stats", ["id": session.coreId], as: Stats.self)
                    (prev, cur) = (cur, s)
                } catch {
                    self.error = error.localizedDescription
                }
                try? await Task.sleep(for: .seconds(every))
            }
        }
    }

    private var cpu: Double? {
        guard let a = prev, let b = cur, b.cpuTotal > a.cpuTotal else { return nil }
        let busy = Double((b.cpuTotal - a.cpuTotal) - min(b.cpuIdle - min(a.cpuIdle, b.cpuIdle), b.cpuTotal - a.cpuTotal))
        return busy / Double(b.cpuTotal - a.cpuTotal)
    }

    private func ratio(_ used: UInt64, _ total: UInt64) -> Double? {
        total == 0 ? nil : Double(used) / Double(total)
    }

    private func bytes(_ kb: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(kb) * 1024, countStyle: .memory)
    }

    private func rate(_ key: KeyPath<Stats, UInt64>) -> String {
        guard let a = prev, let b = cur, b[keyPath: key] >= a[keyPath: key] else { return "…" }
        let perSec = Double(b[keyPath: key] - a[keyPath: key]) / every
        return ByteCountFormatter.string(fromByteCount: Int64(perSec), countStyle: .binary) + "/s"
    }

    private func meter(_ label: String, _ value: Double?, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            LabeledContent(label, value: detail)
            ProgressView(value: value ?? 0)
                .tint((value ?? 0) > 0.9 ? .red : (value ?? 0) > 0.75 ? .orange : .accentColor)
        }
        .accessibilityElement(children: .combine)
    }
}
