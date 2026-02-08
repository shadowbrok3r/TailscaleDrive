import SwiftUI
import QuartzCore
import Metal
import UIKit
import UserNotifications

struct ContentView: View {
    var body: some View {
        EguiView()
            .ignoresSafeArea()
    }
}

// ── MetalHostView — UIKeyInput for iOS keyboard support ─────────────────

final class MetalHostView: UIView, UIKeyInput {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    // Touch callbacks
    var onTouch: ((UITouch.Phase, CGPoint) -> Void)?

    // Keyboard callbacks (wired up by Coordinator)
    var onTextInput: ((String) -> Void)?
    var onDeleteBackward: (() -> Void)?

    // ── UIKeyInput ──────────────────────────────────────────────────

    override var canBecomeFirstResponder: Bool { true }

    var hasText: Bool { true }

    func insertText(_ text: String) {
        onTextInput?(text)
    }

    func deleteBackward() {
        onDeleteBackward?()
    }

    // Prevent the view from showing any native text selection UI
    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        return false
    }

    // ── Touch handling ──────────────────────────────────────────────

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        onTouch?(.began, t.location(in: self))
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        onTouch?(.moved, t.location(in: self))
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        onTouch?(.ended, t.location(in: self))
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        onTouch?(.cancelled, t.location(in: self))
    }
}

// ── EguiView — UIViewRepresentable ──────────────────────────────────────

struct EguiView: UIViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> MetalHostView {
        let v = MetalHostView()

        // Only set the content scale — let wgpu fully own the CAMetalLayer
        v.contentScaleFactor = UIScreen.main.scale
        v.metalLayer.contentsScale = UIScreen.main.scale

        context.coordinator.start(view: v)
        return v
    }

    func updateUIView(_ uiView: MetalHostView, context: Context) {
        let scale = uiView.contentScaleFactor
        let wPx = UInt32(uiView.bounds.size.width * scale)
        let hPx = UInt32(uiView.bounds.size.height * scale)
        if wPx == 0 || hPx == 0 { return }

        uiView.metalLayer.drawableSize = CGSize(width: Int(wPx), height: Int(hPx))
        context.coordinator.resizeIfNeeded(view: uiView)
    }

    @MainActor
    final class Coordinator: NSObject, UNUserNotificationCenterDelegate {
        private var renderer: RendererHandle?
        private var link: CADisplayLink?
        private var t0 = CACurrentMediaTime()
        private weak var hostView: MetalHostView?
        private var keyboardShown = false

        override init() {
            super.init()
            UNUserNotificationCenter.current().delegate = self
            requestNotificationPermissions()
        }

        func start(view: MetalHostView) {
            let scale = view.contentScaleFactor
            let wPx = UInt32(view.bounds.size.width * scale)
            let hPx = UInt32(view.bounds.size.height * scale)
            let ppp = Float(scale)

            // Wait for layout if needed
            if wPx == 0 || hPx == 0 {
                DispatchQueue.main.async { [weak self, weak view] in
                    guard let self, let view else { return }
                    self.start(view: view)
                }
                return
            }

            view.metalLayer.drawableSize = CGSize(width: Int(wPx), height: Int(hPx))

            let layerPtr = UnsafeMutableRawPointer(Unmanaged.passUnretained(view.metalLayer).toOpaque())
            renderer = RendererHandle(layerPtr, wPx, hPx, ppp)
            hostView = view

            // Tell Rust where to save downloaded / pulled files
            do {
                let dlDir = try AppFiles.downloadsDirectory()
                renderer?.setSaveDirectory(dlDir.path)
            } catch {
                print("[Storage] Failed to set Downloads directory: \(error)")
            }


            // ── Wire up touch callbacks ──
            view.onTouch = { [weak self] phase, pt in
                guard let self, let r = self.renderer else { return }
                let x = Float(pt.x)
                let y = Float(pt.y)
                switch phase {
                case .began: r.touchBegan(x, y)
                case .moved: r.touchMoved(x, y)
                case .ended, .cancelled: r.touchEnded(x, y)
                default: break
                }
            }

            // ── Wire up keyboard callbacks ──
            view.onTextInput = { [weak self] text in
                self?.renderer?.sendTextInput(text)
            }

            view.onDeleteBackward = { [weak self] in
                self?.renderer?.sendDeleteBackward()
            }

            let dl = CADisplayLink(target: self, selector: #selector(tick))
            dl.add(to: .main, forMode: .common)
            link = dl
        }

        func resizeIfNeeded(view: MetalHostView) {
            guard let r = renderer else { return }

            let scale = view.contentScaleFactor
            let wPx = UInt32(view.bounds.size.width * scale)
            let hPx = UInt32(view.bounds.size.height * scale)
            if wPx == 0 || hPx == 0 { return }

            r.setPixelsPerPoint(Float(scale))
            r.resize(wPx, hPx)
        }

        @objc private func tick() {
            let t = CACurrentMediaTime() - t0
            renderer?.render(t)

            // ── Keyboard show/hide ──
            updateKeyboardVisibility()

            // ── Share sheet for saved files ──
            pollPendingShares()

            // ── Notification polling ──
            pollNotifications()
        }

        // ── Keyboard management ────────────────────────────────────

        private func updateKeyboardVisibility() {
            guard let r = renderer, let view = hostView else { return }

            let wantsKB = r.wantsKeyboard()

            if wantsKB && !keyboardShown {
                // egui text edit gained focus — show iOS keyboard
                keyboardShown = true
                view.becomeFirstResponder()
            } else if !wantsKB && keyboardShown {
                // egui text edit lost focus — hide iOS keyboard
                keyboardShown = false
                view.resignFirstResponder()
            }
        }

        // ── Share sheet for saved files ──────────────────────────────

        private var shareSheetPresented = false

        private func pollPendingShares() {
            guard let r = renderer, !shareSheetPresented else { return }

            if r.hasPendingShare() {
                let path = r.consumePendingSharePath()
                if !path.isEmpty {
                    presentShareSheet(filePath: path)
                }
            }
        }

        private func presentShareSheet(filePath: String) {
            let fileURL = URL(fileURLWithPath: filePath)
            guard FileManager.default.fileExists(atPath: filePath) else { return }

            shareSheetPresented = true
            let activityVC = UIActivityViewController(
                activityItems: [fileURL],
                applicationActivities: nil
            )

            // Find the root view controller to present from
            guard let windowScene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
                  let rootVC = windowScene.windows.first?.rootViewController else {
                shareSheetPresented = false
                return
            }

            activityVC.completionWithItemsHandler = { [weak self] _, _, _, _ in
                self?.shareSheetPresented = false
            }

            // On iPad, the activity controller needs a popover source
            if let popover = activityVC.popoverPresentationController {
                popover.sourceView = rootVC.view
                popover.sourceRect = CGRect(x: rootVC.view.bounds.midX, y: rootVC.view.bounds.midY, width: 0, height: 0)
                popover.permittedArrowDirections = []
            }

            rootVC.present(activityVC, animated: true)
        }

        // ── Notification handling ──────────────────────────────────

        private func requestNotificationPermissions() {
            UNUserNotificationCenter.current().requestAuthorization(
                options: [.alert, .sound, .badge]
            ) { granted, error in
                if let error = error {
                    print("[Notifications] Permission error: \(error)")
                }
                if granted {
                    print("[Notifications] Permission granted")
                }
            }
        }

        private func pollNotifications() {
            guard let r = renderer else { return }

            while r.hasPendingNotification() {
                let title = r.notificationTitle()
                let body = r.consumeNotificationBody()

                if !title.isEmpty || !body.isEmpty {
                    sendLocalNotification(title: title, body: body)
                }
            }
        }

        private func sendLocalNotification(title: String, body: String) {
            let content = UNMutableNotificationContent()
            content.title = title
            content.body = body
            content.sound = .default

            let request = UNNotificationRequest(
                identifier: UUID().uuidString,
                content: content,
                trigger: nil
            )
            UNUserNotificationCenter.current().add(request) { error in
                if let error = error {
                    print("[Notifications] Failed to send: \(error)")
                }
            }
        }

        // ── UNUserNotificationCenterDelegate ───────────────────────

        nonisolated func userNotificationCenter(
            _ center: UNUserNotificationCenter,
            willPresent notification: UNNotification,
            withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
        ) {
            completionHandler([.banner, .sound, .badge])
        }

        nonisolated func userNotificationCenter(
            _ center: UNUserNotificationCenter,
            didReceive response: UNNotificationResponse,
            withCompletionHandler completionHandler: @escaping () -> Void
        ) {
            completionHandler()
        }
    }
}
