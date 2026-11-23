import SwiftTerm
import SwiftUI
import UIKit

/// One SSH shell and its terminal view, kept alive while the user moves around the app.
@MainActor @Observable
final class TermSession: Identifiable {
    enum Phase: Equatable { case connecting, open, closed(String?) }

    let id = UUID().uuidString
    let host: Host
    /// The tmux session this shell runs in, attached again on every reconnect; nil for a plain shell.
    let tmux: String?
    var phase: Phase = .connecting
    var askPassword = false
    /// Server stats of the panel, kept across its tabs.
    let stats = StatsFeed()
    /// The connection was lost (rather than ended by the server): it comes back on its own.
    @ObservationIgnored private(set) var dropped = false
    /// The core's id for the current connection: new on each reconnect, so a late event of the old one can't hit it.
    @ObservationIgnored private(set) var coreId = ""
    @ObservationIgnored let view: TerminalView
    @ObservationIgnored weak var model: AppModel?
    @ObservationIgnored private var size = (cols: 80, rows: 24)
    @ObservationIgnored private var lastAutoReconnect = Date.distantPast
    @ObservationIgnored private var pinch: Pinch?

    init(host: Host, tmux: String?, theme: Theme, font: UIFont) {
        self.host = host
        self.tmux = tmux
        view = TerminalView(frame: CGRect(x: 0, y: 0, width: 390, height: 600), font: font)
        view.terminalDelegate = self
        view.inputAccessoryView = KeyBar(terminal: view)
        // Two-finger pinch sets the text size of every terminal, like the toolbar's buttons.
        pinch = Pinch(view) { [weak self] scale, began in self?.model?.pinch(scale, began: began) }
        apply(theme)
    }

    func apply(_ t: Theme) {
        view.nativeBackgroundColor = t.bg
        view.nativeForegroundColor = t.fg
        view.caretColor = t.cursor
        view.selectedTextBackgroundColor = t.selection
        view.installColors(t.ansi.map { c in
            var (r, g, b): (CGFloat, CGFloat, CGFloat) = (0, 0, 0)
            c.getRed(&r, green: &g, blue: &b, alpha: nil)
            return SwiftTerm.Color(red: UInt16(r * 65535), green: UInt16(g * 65535), blue: UInt16(b * 65535))
        })
    }

    func connect(password: String? = nil, remember: Bool = false) async {
        phase = .connecting
        dropped = false
        coreId = UUID().uuidString
        var args: [String: Any] = [
            "session": coreId, "host_id": host.id, "cols": size.cols, "rows": size.rows,
            "password": password ?? NSNull(), "tmux": tmux ?? NSNull(),
        ]
        if host.isQuick { args["host"] = Core.object(host) }
        do {
            try await Core.call("ssh_connect", args)
            phase = .open
            // The view may have been laid out while connecting.
            _ = try? await Core.call("ssh_resize", ["id": coreId, "cols": size.cols, "rows": size.rows])
            if remember, let password { _ = try? await Core.call("set_secret", ["id": host.id, "value": password]) }
        } catch let e as Core.Failure where e.message.contains("NEED_PASSWORD") {
            askPassword = true
        } catch {
            phase = .closed(error.localizedDescription)
        }
    }

    /// After a lost connection: once per 30 s at most, so a server that keeps dropping us doesn't loop.
    func reconnectIfDropped() {
        guard dropped, Date.now.timeIntervalSince(lastAutoReconnect) > 30 else { return }
        lastAutoReconnect = .now
        view.feed(text: "\r\n\u{1b}[2m— reconnexion —\u{1b}[0m\r\n")
        Task { await connect() }
    }

    /// A window change makes the core write to the socket: one iOS closed while the app was away reports itself now.
    func checkAlive() {
        guard phase == .open else { return }
        Task { _ = try? await Core.call("ssh_resize", ["id": coreId, "cols": size.cols, "rows": size.rows]) }
    }

    func close() {
        let id = coreId
        Task { try? await Core.call("ssh_close", ["id": id]) }
        phase = .closed(nil)
    }

    func feed(_ data: Data) {
        view.feed(byteArray: ArraySlice(data))
    }

    /// `ended`: the server closed the shell (exit); otherwise the network dropped.
    func closed(ended: Bool) {
        guard phase == .open else { return }
        dropped = !ended
        phase = .closed(ended ? nil : "Connexion perdue")
        view.feed(text: "\r\n")
    }
}

extension TermSession: TerminalViewDelegate {
    func send(source: TerminalView, data: ArraySlice<UInt8>) {
        if phase == .open { Core.write(coreId, data) }
    }

    func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        size = (newCols, newRows)
        if phase == .open {
            Task { try? await Core.call("ssh_resize", ["id": coreId, "cols": newCols, "rows": newRows]) }
        }
    }

    func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        if let url = URL(string: link), ["http", "https"].contains(url.scheme) { UIApplication.shared.open(url) }
    }

    func bell(source: TerminalView) {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    func clipboardCopy(source: TerminalView, content: Data) {
        UIPasteboard.general.string = String(decoding: content, as: UTF8.self)
    }

    func clipboardRead(source: TerminalView) -> Data? {
        UIPasteboard.general.string.map { Data($0.utf8) }
    }

    func setTerminalTitle(source: TerminalView, title: String) {}
    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func scrolled(source: TerminalView, position: Double) {}
    func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
    func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
}

/// UIKit's target/action for the pinch, as a closure: (scale since the gesture began, began).
private final class Pinch: NSObject {
    private let onChange: (CGFloat, Bool) -> Void

    init(_ view: UIView, _ onChange: @escaping (CGFloat, Bool) -> Void) {
        self.onChange = onChange
        super.init()
        view.addGestureRecognizer(UIPinchGestureRecognizer(target: self, action: #selector(run)))
    }

    @objc private func run(_ g: UIPinchGestureRecognizer) {
        if g.state == .began || g.state == .changed { onChange(g.scale, g.state == .began) }
    }
}

/// Hosts a session's TerminalView (the same instance across screens, so the scrollback survives).
struct TerminalHost: UIViewRepresentable {
    let session: TermSession

    func makeUIView(context: Context) -> UIView {
        let box = UIView()
        session.view.removeFromSuperview()
        session.view.frame = box.bounds
        session.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        box.addSubview(session.view)
        return box
    }

    func updateUIView(_ box: UIView, context: Context) {}
}
