import Charts
import SwiftUI

/// Server stats (Linux /proc, like the desktop's right panel), refreshed every 2 s while shown: gauges for
/// processor, memory and disk, the last two minutes of processor and network as charts.
struct StatsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dynamicTypeSize) private var typeSize
    let session: TermSession
    private let every = 2.0
    private var feed: StatsFeed { session.stats }
    private var cur: Stats? { feed.cur }
    private var history: [StatsFeed.Sample] { feed.history }
    private var cpu: Double? { feed.cpu }

    var body: some View {
        ScrollView {
            VStack(spacing: 12) {
                if let s = cur {
                    system(s)
                    gauges(s)
                    cpuCard
                    network
                    load(s)
                } else if let error = feed.error {
                    ContentUnavailableView("Statistiques indisponibles", systemImage: "exclamationmark.triangle", description: Text(error))
                } else {
                    ProgressView().padding(.top, 60)
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 4)
            .padding(.bottom, 24)
        }
        .monospacedDigit()
        .themedBackground()
        .task {
            while !Task.isCancelled {
                do {
                    feed.add(try await Core.call("ssh_stats", ["id": session.coreId], as: Stats.self))
                } catch {
                    feed.error = error.localizedDescription
                }
                try? await Task.sleep(for: .seconds(every))
            }
        }
    }

    private var accent: Color { model.theme.tint.map { Color($0) } ?? .accentColor }

    // MARK: cards

    private func system(_ s: Stats) -> some View {
        PanelCard {
            HStack(spacing: 12) {
                IconTile(systemImage: "server.rack")
                VStack(alignment: .leading, spacing: 2) {
                    Text(s.hostname).font(.headline).lineLimit(1)
                    Text(s.osName).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                }
            }
            Divider()
            HStack(alignment: .top, spacing: 16) {
                info("Noyau", s.kernel)
                info("Allumé depuis", Duration.seconds(s.uptimeSecs).formatted(.units(allowed: [.days, .hours, .minutes], width: .abbreviated, maximumUnitCount: 2)))
            }
        }
    }

    private func info(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.subheadline.monospaced()).lineLimit(1).minimumScaleFactor(0.7)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private func gauges(_ s: Stats) -> some View {
        let used = s.memTotalKb - min(s.memAvailKb, s.memTotalKb)
        // Three side by side; stacked at the accessibility text sizes, where they would not fit.
        let layout = typeSize.isAccessibilitySize ? AnyLayout(VStackLayout(spacing: 10)) : AnyLayout(HStackLayout(spacing: 10))
        return layout {
            Ring(label: "Processeur", value: cpu, detail: history.count > 1 ? "moyenne " + average.formatted(.percent.precision(.fractionLength(0))) : "")
            Ring(label: "Mémoire", value: ratio(used, s.memTotalKb), detail: "\(bytes(used)) / \(bytes(s.memTotalKb))")
            Ring(label: "Disque /", value: ratio(s.diskUsedKb, s.diskTotalKb), detail: "\(bytes(s.diskUsedKb)) / \(bytes(s.diskTotalKb))")
        }
    }

    private var cpuCard: some View {
        PanelCard(title: "Processeur", systemImage: "cpu") {
            Text(cpu.map { $0.formatted(.percent.precision(.fractionLength(0))) } ?? "…").font(.subheadline.weight(.semibold))
        } content: {
            Chart(history) { p in
                AreaMark(x: .value("Temps", p.id), y: .value("Processeur", p.cpu))
                    .foregroundStyle(LinearGradient(colors: [accent.opacity(0.35), accent.opacity(0.02)], startPoint: .top, endPoint: .bottom))
                    .interpolationMethod(.monotone)
                LineMark(x: .value("Temps", p.id), y: .value("Processeur", p.cpu))
                    .foregroundStyle(accent)
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round))
                    .interpolationMethod(.monotone)
            }
            .chartXScale(domain: xDomain)
            .chartXAxis(.hidden)
            .chartYScale(domain: 0...1)
            .chartYAxis {
                AxisMarks(position: .trailing, values: [0, 0.5, 1]) { v in
                    AxisGridLine().foregroundStyle(.secondary.opacity(0.3))
                    AxisValueLabel { Text((v.as(Double.self) ?? 0).formatted(.percent)) }
                }
            }
            .frame(height: 110)
            .accessibilityLabel("Processeur sur les deux dernières minutes")
        }
    }

    private var network: some View {
        PanelCard(title: "Réseau", systemImage: "arrow.up.arrow.down") {
            HStack(spacing: 16) {
                rateView("Réception", "arrow.down", feed.rx, accent)
                rateView("Envoi", "arrow.up", feed.tx, .orange)
            }
            Chart(history) { p in
                LineMark(x: .value("Temps", p.id), y: .value("Débit", p.rx), series: .value("Sens", "Réception"))
                    .foregroundStyle(accent)
                    .interpolationMethod(.monotone)
                LineMark(x: .value("Temps", p.id), y: .value("Débit", p.tx), series: .value("Sens", "Envoi"))
                    .foregroundStyle(.orange)
                    .interpolationMethod(.monotone)
            }
            .chartXScale(domain: xDomain)
            .chartXAxis(.hidden)
            .chartYAxis(.hidden)
            .frame(height: 70)
            .accessibilityLabel("Réception et envoi sur les deux dernières minutes")
        }
    }

    private func rateView(_ label: String, _ symbol: String, _ perSec: Double?, _ color: Color) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(label, systemImage: symbol).font(.caption.weight(.medium)).foregroundStyle(color)
            Text(perSec.map { ByteCountFormatter.string(fromByteCount: Int64($0), countStyle: .binary) + "/s" } ?? "…").font(.title3.weight(.semibold)).lineLimit(1).minimumScaleFactor(0.7)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private func load(_ s: Stats) -> some View {
        PanelCard(title: "Charge moyenne", systemImage: "chart.bar") {
            HStack(spacing: 8) {
                ForEach(Array(zip(["1 min", "5 min", "15 min"], s.load)), id: \.0) { label, v in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(v.formatted(.number.precision(.fractionLength(2)))).font(.title3.weight(.semibold))
                        Text(label).font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    // MARK: values

    /// Fixed two-minute window, so the curves scroll instead of stretching.
    private var xDomain: ClosedRange<Int> {
        let last = max(history.last?.id ?? 0, StatsFeed.keep - 1)
        return (last - StatsFeed.keep + 1)...last
    }

    private var average: Double {
        history.isEmpty ? 0 : history.map(\.cpu).reduce(0, +) / Double(history.count)
    }

    private func ratio(_ used: UInt64, _ total: UInt64) -> Double? {
        total == 0 ? nil : Double(used) / Double(total)
    }

    private func bytes(_ kb: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(kb) * 1024, countStyle: .memory)
    }
}

/// A session's stats and their last two minutes, kept by the session so switching the panel's tab (or closing
/// it) doesn't start them over. Sampled only while the Stats tab is shown.
@MainActor @Observable
final class StatsFeed {
    struct Sample: Identifiable {
        let id: Int
        let cpu: Double, rx: Double, tx: Double
    }
    /// 2 minutes at one sample every 2 s.
    static let keep = 60

    private(set) var cur: Stats?
    private(set) var history: [Sample] = []
    /// Processor use and network rates between the last two samples.
    private(set) var cpu: Double?
    private(set) var rx: Double?
    private(set) var tx: Double?
    var error: String?
    @ObservationIgnored private var at = Date.distantPast

    func add(_ s: Stats) {
        let now = Date.now
        if let p = cur {
            // Real elapsed time: after a pause (tab hidden), rates are the average since the last sample.
            let secs = max(now.timeIntervalSince(at), 0.5)
            let rate = { (a: UInt64, b: UInt64) in b >= a ? Double(b - a) / secs : nil }
            (rx, tx) = (rate(p.netRx, s.netRx), rate(p.netTx, s.netTx))
            if s.cpuTotal > p.cpuTotal {
                let total = s.cpuTotal - p.cpuTotal
                cpu = Double(total - min(s.cpuIdle - min(p.cpuIdle, s.cpuIdle), total)) / Double(total)
            } else {
                cpu = nil // counters went back: the server rebooted
            }
            if let c = cpu {
                history.append(Sample(id: (history.last?.id ?? -1) + 1, cpu: c, rx: rx ?? 0, tx: tx ?? 0))
                if history.count > Self.keep { history.removeFirst(history.count - Self.keep) }
            }
        }
        (cur, at, error) = (s, now, nil)
    }
}

/// A usage ring: tint, orange above 75 %, red above 90 % (the percentage is always written in the middle).
private struct Ring: View {
    let label: String
    let value: Double?
    let detail: String
    @ScaledMetric(relativeTo: .body) private var size = 64

    var body: some View {
        let v = min(max(value ?? 0, 0), 1)
        let level: AnyShapeStyle = v > 0.9 ? AnyShapeStyle(.red) : v > 0.75 ? AnyShapeStyle(.orange) : AnyShapeStyle(.tint)
        let percent = value.map { $0.formatted(.percent.precision(.fractionLength(0))) } ?? "…"
        VStack(spacing: 8) {
            ZStack {
                Circle().stroke(.primary.opacity(0.1), lineWidth: size * 0.12)
                Circle()
                    .trim(from: 0, to: v)
                    .stroke(level, style: StrokeStyle(lineWidth: size * 0.12, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                    .animation(.smooth, value: v)
                Text(percent).font(.subheadline.weight(.bold)).minimumScaleFactor(0.6).padding(size * 0.16)
            }
            .frame(width: size, height: size)
            Text(label).font(.footnote.weight(.semibold)).lineLimit(1)
            Text(detail).font(.caption2).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.6)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 14)
        .padding(.horizontal, 6)
        .background(panelFill, in: .rect(cornerRadius: 22))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityValue(detail.isEmpty || detail == "…" ? percent : "\(percent), \(detail)")
    }
}
