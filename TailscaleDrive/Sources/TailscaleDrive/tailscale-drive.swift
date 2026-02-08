import BridgeFFI

public func renderer_new(_ layer_ptr: UnsafeMutableRawPointer, _ width_px: UInt32, _ height_px: UInt32, _ pixels_per_point: Float) -> UnsafeMutableRawPointer {
    __swift_bridge__$renderer_new(layer_ptr, width_px, height_px, pixels_per_point)
}
public func renderer_free(_ ptr: UnsafeMutableRawPointer) {
    __swift_bridge__$renderer_free(ptr)
}
public func renderer_resize(_ ptr: UnsafeMutableRawPointer, _ width_px: UInt32, _ height_px: UInt32) {
    __swift_bridge__$renderer_resize(ptr, width_px, height_px)
}
public func renderer_set_pixels_per_point(_ ptr: UnsafeMutableRawPointer, _ pixels_per_point: Float) {
    __swift_bridge__$renderer_set_pixels_per_point(ptr, pixels_per_point)
}
public func renderer_render(_ ptr: UnsafeMutableRawPointer, _ time_seconds: Double) {
    __swift_bridge__$renderer_render(ptr, time_seconds)
}
public func renderer_touch_began(_ ptr: UnsafeMutableRawPointer, _ x_pt: Float, _ y_pt: Float) {
    __swift_bridge__$renderer_touch_began(ptr, x_pt, y_pt)
}
public func renderer_touch_moved(_ ptr: UnsafeMutableRawPointer, _ x_pt: Float, _ y_pt: Float) {
    __swift_bridge__$renderer_touch_moved(ptr, x_pt, y_pt)
}
public func renderer_touch_ended(_ ptr: UnsafeMutableRawPointer, _ x_pt: Float, _ y_pt: Float) {
    __swift_bridge__$renderer_touch_ended(ptr, x_pt, y_pt)
}
public func renderer_has_pending_notification(_ ptr: UnsafeMutableRawPointer) -> Bool {
    __swift_bridge__$renderer_has_pending_notification(ptr)
}
public func renderer_notification_title(_ ptr: UnsafeMutableRawPointer) -> RustString {
    RustString(ptr: __swift_bridge__$renderer_notification_title(ptr))
}
public func renderer_consume_notification_body(_ ptr: UnsafeMutableRawPointer) -> RustString {
    RustString(ptr: __swift_bridge__$renderer_consume_notification_body(ptr))
}
public func renderer_wants_keyboard(_ ptr: UnsafeMutableRawPointer) -> Bool {
    __swift_bridge__$renderer_wants_keyboard(ptr)
}
public func renderer_insert_text<GenericIntoRustString: IntoRustString>(_ ptr: UnsafeMutableRawPointer, _ text: GenericIntoRustString) {
    __swift_bridge__$renderer_insert_text(ptr, { let rustString = text.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
}
public func renderer_delete_backward(_ ptr: UnsafeMutableRawPointer) {
    __swift_bridge__$renderer_delete_backward(ptr)
}
public func renderer_set_save_directory<GenericIntoRustString: IntoRustString>(_ ptr: UnsafeMutableRawPointer, _ path: GenericIntoRustString) {
    __swift_bridge__$renderer_set_save_directory(ptr, { let rustString = path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
}
public func renderer_has_pending_share(_ ptr: UnsafeMutableRawPointer) -> Bool {
    __swift_bridge__$renderer_has_pending_share(ptr)
}
public func renderer_consume_pending_share_path(_ ptr: UnsafeMutableRawPointer) -> RustString {
    RustString(ptr: __swift_bridge__$renderer_consume_pending_share_path(ptr))
}


