import SwiftUI
import Tauri
import UIKit
import WebKit

struct AttachArgs: Decodable { let events: Channel }
struct ChromeArgs: Decodable { let state: Chrome }
struct MenuArgs: Decodable { let x: Double; let y: Double; let items: [Item] }
struct SheetArgs: Decodable { let sheet: SheetDoc }
struct IdArgs: Decodable { let id: Int }
struct ToastArgs: Decodable { let text: String; let kind: String }

/// Height of the title bar below the status bar; the page leaves the same room (style.css).
let barHeight: CGFloat = 44

/// What WKContentView returns as its keyboard accessory (swizzled below): the key bar while a terminal has the
/// keyboard, else nothing (no WebKit "‹ › Done" bar).
private var keyAccessory: UIView?

/// The chrome of the iOS app drawn in SwiftUI over the webview: title bar, key bar, menus, sheets and toasts.
/// The page keeps the logic; it sends what to show and gets back what the user did (src/native.js).
@available(iOS 26.0, *)
final class NativeUIPlugin: Plugin {
  private weak var webview: WKWebView?
  private var root: UIViewController?
  private var events: Channel?
  private let chrome = ChromeModel()
  private var bar: UIHostingController<TopBar>?
  private var keys: UIHostingController<KeyBar>?
  private var sheets: [Int: (host: SheetHost, model: SheetModel)] = [:]
  private var order: [Int] = []
  private let toastModel = ToastModel()
  private var toastHost: UIHostingController<ToastView>?
  private var toastTimer: DispatchWorkItem?
  private var style: UIUserInterfaceStyle = .dark

  override func load(webview: WKWebView) { self.webview = webview }

  private func send(_ data: JsonObject) { events?.send(data) }

  // Commands run on Tauri's IPC queue: UIKit work goes to the main thread.

  @objc public func attach(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(AttachArgs.self)
    DispatchQueue.main.async {
      self.events = args.events
      self.install()
      invoke.resolve()
    }
  }

  @objc public func chrome(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ChromeArgs.self)
    DispatchQueue.main.async {
      let typing = args.state.typing != self.chrome.state.typing
      self.chrome.state = args.state
      // The theme's light or dark look, for the native parts only: the page keeps following the system.
      self.style = args.state.dark ? .dark : .light
      self.bar?.overrideUserInterfaceStyle = self.style
      self.keys?.overrideUserInterfaceStyle = self.style
      if typing {
        keyAccessory = args.state.typing ? self.keys?.view : nil
        self.contentView()?.reloadInputViews()
      }
      invoke.resolve()
    }
  }

  @objc public func menu(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(MenuArgs.self)
    DispatchQueue.main.async {
      guard let webview = self.webview, let top = self.top() else { return invoke.resolve(["index": -1]) }
      var done = false
      let finish = { (i: Int) in
        if !done { done = true; invoke.resolve(["index": i]) }
      }
      let pop = Popover(rootView: AnyView(EmptyView()))
      pop.rootView = AnyView(MenuList(items: args.items) { [weak pop] i in
        // Answer once the menu is gone: the action may present an alert or a sheet from the same controller.
        pop?.dismiss(animated: true) { finish(i) }
      }.tint(Color(hex: self.chrome.state.tint)))
      pop.onDismiss = { finish(-1) }
      pop.modalPresentationStyle = .popover
      pop.overrideUserInterfaceStyle = self.style
      pop.view.backgroundColor = .clear
      if let pc = pop.popoverPresentationController {
        pc.sourceView = webview
        pc.sourceRect = CGRect(x: args.x, y: args.y, width: 1, height: 1)
        pc.delegate = pop
      }
      pop.preferredContentSize = pop.sizeThatFits(in: CGSize(width: 250, height: 2000))
      top.present(pop, animated: true)
    }
  }

  @objc public func sheet(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SheetArgs.self)
    DispatchQueue.main.async {
      defer { invoke.resolve() }
      let doc = args.sheet, id = doc.id
      if let open = self.sheets[id] { return open.model.update(doc) }
      let model = SheetModel(doc)
      let host = SheetHost(rootView: SheetView(model: model, tint: Color(hex: self.chrome.state.tint)) { [weak self] data in
        var d = data
        d["sheet"] = id
        self?.send(d)
      })
      host.onDismiss = { [weak self] in self?.closed(id, byUser: true) }
      host.overrideUserInterfaceStyle = self.style
      if let sc = host.sheetPresentationController {
        sc.detents = doc.large ? [.large()] : [.medium(), .large()]
        sc.prefersGrabberVisible = !doc.large
      }
      host.presentationController?.delegate = host
      self.top()?.present(host, animated: true)
      self.sheets[id] = (host, model)
      self.order.append(id)
      self.manager.viewController = host // the dialog plugin (file pickers, confirmations) presents over the sheet
    }
  }

  @objc public func dismiss(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    DispatchQueue.main.async {
      self.closed(args.id, byUser: false)
      invoke.resolve()
    }
  }

  @objc public func toast(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ToastArgs.self)
    DispatchQueue.main.async {
      self.showToast(args.text, args.kind)
      invoke.resolve()
    }
  }

  // MARK: -

  private func install() {
    guard root == nil, let root = manager.viewController, let view = root.view else { return }
    self.root = root
    let bar = UIHostingController(rootView: TopBar(model: chrome) { [weak self] in self?.send($0) })
    bar.view.backgroundColor = .clear
    root.addChild(bar)
    view.addSubview(bar.view)
    bar.didMove(toParent: root)
    self.bar = bar
    bar.view.translatesAutoresizingMaskIntoConstraints = false
    NSLayoutConstraint.activate([
      bar.view.topAnchor.constraint(equalTo: view.topAnchor),
      bar.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      bar.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
      bar.view.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: barHeight),
    ])
    let keys = UIHostingController(rootView: KeyBar(model: chrome) { [weak self] in self?.send($0) })
    keys.view.backgroundColor = .clear
    keys.view.frame = CGRect(x: 0, y: 0, width: view.bounds.width, height: 52)
    keys.view.autoresizingMask = .flexibleWidth
    self.keys = keys
    Self.swizzleAccessory()
  }

  /// WKWebView has no public way to change the bar above the keyboard: WKContentView's getter is replaced.
  private static func swizzleAccessory() {
    let sel = #selector(getter: UIResponder.inputAccessoryView)
    guard let cls = NSClassFromString("WKContentView"), let method = class_getInstanceMethod(cls, sel) else { return }
    let getter: @convention(block) (AnyObject) -> UIView? = { _ in keyAccessory }
    class_replaceMethod(cls, sel, imp_implementationWithBlock(getter), method_getTypeEncoding(method))
  }

  private func contentView() -> UIView? {
    webview?.scrollView.subviews.first { NSStringFromClass(type(of: $0)).hasPrefix("WKContentView") }
  }

  /// Where to present: the last controller shown (a sheet, or the app's own).
  private func top() -> UIViewController? {
    var vc = root
    while let next = vc?.presentedViewController, !next.isBeingDismissed { vc = next }
    return vc
  }

  private func closed(_ id: Int, byUser: Bool) {
    guard let sheet = sheets.removeValue(forKey: id) else { return }
    order.removeAll { $0 == id }
    let below: UIViewController? = order.last.flatMap { sheets[$0]?.host }
    manager.viewController = below ?? root
    if byUser { send(["sheet": id, "dismissed": true]) } else { sheet.host.dismiss(animated: true) }
  }

  /// Above everything, sheets included (added to the window, not presented).
  private func showToast(_ text: String, _ kind: String) {
    guard let window = webview?.window else { return }
    if toastHost == nil {
      let host = UIHostingController(rootView: ToastView(model: toastModel))
      host.sizingOptions = .intrinsicContentSize
      host.view.backgroundColor = .clear
      host.view.isUserInteractionEnabled = false
      host.view.translatesAutoresizingMaskIntoConstraints = false
      window.addSubview(host.view)
      NSLayoutConstraint.activate([
        host.view.topAnchor.constraint(equalTo: window.safeAreaLayoutGuide.topAnchor, constant: 6),
        host.view.centerXAnchor.constraint(equalTo: window.centerXAnchor),
        host.view.widthAnchor.constraint(lessThanOrEqualTo: window.widthAnchor, constant: -32),
      ])
      toastHost = host
    }
    guard let host = toastHost else { return }
    host.overrideUserInterfaceStyle = style
    window.bringSubviewToFront(host.view)
    toastModel.text = text
    toastModel.kind = kind
    withAnimation(.spring(duration: 0.35)) { toastModel.shown = true }
    toastTimer?.cancel()
    let hide = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.25)) { self?.toastModel.shown = false } }
    toastTimer = hide
    DispatchQueue.main.asyncAfter(deadline: .now() + (kind == "error" ? 7 : 3.5), execute: hide)
  }
}

/// A menu at a point (long press), as a popover: Liquid Glass, and not a full sheet on iPhone.
final class Popover: UIHostingController<AnyView>, UIPopoverPresentationControllerDelegate {
  var onDismiss: () -> Void = {}

  func adaptivePresentationStyle(for controller: UIPresentationController, traitCollection: UITraitCollection) -> UIModalPresentationStyle { .none }

  func presentationControllerDidDismiss(_ presentationController: UIPresentationController) { onDismiss() }
}

@available(iOS 26.0, *)
final class SheetHost: UIHostingController<SheetView>, UIAdaptivePresentationControllerDelegate {
  var onDismiss: () -> Void = {}

  /// Swiped down: same as Cancel.
  func presentationControllerDidDismiss(_ presentationController: UIPresentationController) { onDismiss() }
}

@_cdecl("init_plugin_native_ui")
public func initPlugin() -> Plugin {
  if #available(iOS 26.0, *) { return NativeUIPlugin() }
  return Plugin()
}
