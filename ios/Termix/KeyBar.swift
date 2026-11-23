import SwiftTerm
import UIKit

/// The row above the keyboard: the keys an iPhone keyboard lacks. Esc, Ctrl and Tab on the left, arrows (held
/// down, they repeat) on the right, the rest scrolls in between. Ctrl and Alt apply to the next key typed.
final class KeyBar: UIInputView, UIInputViewAudioFeedback {
    private weak var terminal: TerminalView?
    private var ctrl: UIButton?
    private var alt: UIButton?
    private var repeatTimer: Timer?
    private let height: CGFloat = UIDevice.current.userInterfaceIdiom == .phone ? 46 : 54

    var enableInputClicksWhenVisible: Bool { true }

    init(terminal: TerminalView) {
        self.terminal = terminal
        super.init(frame: CGRect(x: 0, y: 0, width: 0, height: UIDevice.current.userInterfaceIdiom == .phone ? 46 : 54), inputViewStyle: .keyboard)
        allowsSelfSizing = true
        autoresizingMask = .flexibleWidth
        build()
        // SwiftTerm clears its modifiers once a key used them: follow it.
        for name in [Notification.Name.terminalViewControlModifierReset, .terminalViewMetaModifierReset] {
            NotificationCenter.default.addObserver(self, selector: #selector(syncModifiers), name: name, object: terminal)
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override var intrinsicContentSize: CGSize { CGSize(width: UIView.noIntrinsicMetric, height: height) }

    private func build() {
        let ctrl = key("ctrl", label: "Ctrl") { [weak self] in self?.terminal?.controlModifier.toggle(); self?.syncModifiers() }
        let alt = key("alt", label: "Alt") { [weak self] in self?.terminal?.metaModifier.toggle(); self?.syncModifiers() }
        self.ctrl = ctrl
        self.alt = alt
        let left = [key("esc", label: "Échap") { [weak self] in self?.send([0x1b]) }, ctrl,
                    key(symbol: "arrow.right.to.line", label: "Tab") { [weak self] in self?.send([0x09]) }]
        var middle: [UIButton] = [
            alt,
            key("^C", label: "Ctrl C") { [weak self] in self?.send([0x03]) },
            key("^D", label: "Ctrl D") { [weak self] in self?.send([0x04]) },
            key("^Z", label: "Ctrl Z") { [weak self] in self?.send([0x1a]) },
        ]
        middle += ["~", "|", "/", "-", "_", "*", "&", "$", "\\", "`", "'", "\"", ":", ";", "<", ">", "{", "}", "[", "]"].map { ch in
            key(ch, label: ch) { [weak self] in self?.terminal?.send(txt: ch) }
        }
        middle += [
            key("PgUp", label: "Page précédente") { [weak self] in self?.send(EscapeSequences.cmdPageUp) },
            key("PgDn", label: "Page suivante") { [weak self] in self?.send(EscapeSequences.cmdPageDown) },
        ]
        middle += EscapeSequences.cmdF.prefix(12).enumerated().map { i, seq in
            key("F\(i + 1)", label: "F\(i + 1)") { [weak self] in self?.send(seq) }
        }
        middle.append(key(symbol: "doc.on.clipboard", label: "Coller") { [weak self] in self?.terminal?.paste(nil) })
        let right = [
            repeating(symbol: "arrow.left", label: "Gauche") { [weak self] in self?.terminal?.sendKeyLeft() },
            repeating(symbol: "arrow.down", label: "Bas") { [weak self] in self?.terminal?.sendKeyDown() },
            repeating(symbol: "arrow.up", label: "Haut") { [weak self] in self?.terminal?.sendKeyUp() },
            repeating(symbol: "arrow.right", label: "Droite") { [weak self] in self?.terminal?.sendKeyRight() },
            key(symbol: "keyboard.chevron.compact.down", label: "Masquer le clavier") { [weak self] in _ = self?.terminal?.resignFirstResponder() },
        ]

        let scroll = UIScrollView()
        scroll.showsHorizontalScrollIndicator = false
        scroll.alwaysBounceHorizontal = true
        let inner = stack(middle)
        scroll.addSubview(inner)
        inner.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            inner.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
            inner.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            inner.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            inner.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
            inner.heightAnchor.constraint(equalTo: scroll.frameLayoutGuide.heightAnchor),
        ])
        let row = UIStackView(arrangedSubviews: [stack(left), scroll, stack(right)])
        row.spacing = 6
        row.alignment = .fill
        addSubview(row)
        row.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: safeAreaLayoutGuide.leadingAnchor, constant: 6),
            row.trailingAnchor.constraint(equalTo: safeAreaLayoutGuide.trailingAnchor, constant: -6),
            row.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            row.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -5),
        ])
    }

    private func stack(_ views: [UIView]) -> UIStackView {
        let s = UIStackView(arrangedSubviews: views)
        s.spacing = 6
        s.setContentHuggingPriority(.required, for: .horizontal)
        s.setContentCompressionResistancePriority(.required, for: .horizontal)
        return s
    }

    private func style(_ title: String?, symbol: String?, label: String) -> UIButton {
        var c = UIButton.Configuration.gray()
        c.cornerStyle = .medium
        c.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 9, bottom: 0, trailing: 9)
        if let symbol {
            c.image = UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 14, weight: .medium))
        } else if let title {
            c.attributedTitle = AttributedString(title, attributes: AttributeContainer([.font: UIFont.monospacedSystemFont(ofSize: 15, weight: .medium)]))
        }
        c.baseForegroundColor = .label
        let b = UIButton(configuration: c)
        b.accessibilityLabel = label
        b.widthAnchor.constraint(greaterThanOrEqualToConstant: 36).isActive = true
        return b
    }

    private func key(_ title: String? = nil, symbol: String? = nil, label: String, action: @escaping () -> Void) -> UIButton {
        let b = style(title, symbol: symbol, label: label)
        b.addAction(UIAction { _ in UIDevice.current.playInputClick(); action() }, for: .touchUpInside)
        return b
    }

    /// Sends once on touch down, then repeats like a keyboard key while held.
    private func repeating(symbol: String, label: String, action: @escaping () -> Void) -> UIButton {
        let b = style(nil, symbol: symbol, label: label)
        b.addAction(UIAction { [weak self] _ in
            UIDevice.current.playInputClick()
            action()
            self?.repeatTimer?.invalidate()
            self?.repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { _ in
                MainActor.assumeIsolated {
                    self?.repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.07, repeats: true) { _ in MainActor.assumeIsolated { action() } }
                }
            }
        }, for: .touchDown)
        b.addAction(UIAction { [weak self] _ in self?.repeatTimer?.invalidate(); self?.repeatTimer = nil },
                    for: [.touchUpInside, .touchUpOutside, .touchCancel])
        return b
    }

    private func send(_ bytes: [UInt8]) {
        terminal?.send(bytes)
    }

    @objc private func syncModifiers() {
        for (button, on) in [(ctrl, terminal?.controlModifier == true), (alt, terminal?.metaModifier == true)] {
            guard let button, var c = button.configuration else { continue }
            c.baseBackgroundColor = on ? tintColor : nil
            c.baseForegroundColor = on ? .white : .label
            button.configuration = c
            button.isSelected = on
            button.accessibilityTraits = on ? [.button, .selected] : .button
        }
    }
}
