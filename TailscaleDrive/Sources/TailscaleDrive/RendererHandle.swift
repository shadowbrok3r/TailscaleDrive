import Foundation
import BridgeFFI

final class RendererHandle {
    private var ptr: UnsafeMutableRawPointer

    init(_ layerPtr: UnsafeMutableRawPointer, _ wPx: UInt32, _ hPx: UInt32, _ ppp: Float) {
        self.ptr = renderer_new(layerPtr, wPx, hPx, ppp)
    }

    deinit { renderer_free(ptr) }

    func resize(_ wPx: UInt32, _ hPx: UInt32) { renderer_resize(ptr, wPx, hPx) }
    func setPixelsPerPoint(_ ppp: Float) { renderer_set_pixels_per_point(ptr, ppp) }
    func render(_ t: Double) { renderer_render(ptr, t) }

    func touchBegan(_ xPt: Float, _ yPt: Float) { renderer_touch_began(ptr, xPt, yPt) }
    func touchMoved(_ xPt: Float, _ yPt: Float) { renderer_touch_moved(ptr, xPt, yPt) }
    func touchEnded(_ xPt: Float, _ yPt: Float) { renderer_touch_ended(ptr, xPt, yPt) }

    // Notification polling
    func hasPendingNotification() -> Bool {
        renderer_has_pending_notification(ptr)
    }

    func notificationTitle() -> String {
        renderer_notification_title(ptr).toString()
    }

    /// Returns the body AND consumes the notification from the queue.
    func consumeNotificationBody() -> String {
        renderer_consume_notification_body(ptr).toString()
    }

    // iOS keyboard support
    func wantsKeyboard() -> Bool {
        renderer_wants_keyboard(ptr)
    }

    func sendTextInput(_ text: String) {
        renderer_insert_text(ptr, text)
    }

    func sendDeleteBackward() {
        renderer_delete_backward(ptr)
    }

    // Save directory & share sheet
    func setSaveDirectory(_ path: String) {
        renderer_set_save_directory(ptr, path)
    }

    func hasPendingShare() -> Bool {
        renderer_has_pending_share(ptr)
    }

    func consumePendingSharePath() -> String {
        renderer_consume_pending_share_path(ptr).toString()
    }
}
