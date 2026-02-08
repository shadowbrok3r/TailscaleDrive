use std::ffi::c_void;

mod tailscale_client;
mod renderer;
pub use renderer::Renderer;

#[swift_bridge::bridge]
mod ffi {
    extern "Rust" {
        fn renderer_new(layer_ptr: *mut c_void, width_px: u32, height_px: u32, pixels_per_point: f32) -> *mut c_void;
        fn renderer_free(ptr: *mut c_void);

        fn renderer_resize(ptr: *mut c_void, width_px: u32, height_px: u32);
        fn renderer_set_pixels_per_point(ptr: *mut c_void, pixels_per_point: f32);

        fn renderer_render(ptr: *mut c_void, time_seconds: f64);

        // Touch input (points, not pixels)
        fn renderer_touch_began(ptr: *mut c_void, x_pt: f32, y_pt: f32);
        fn renderer_touch_moved(ptr: *mut c_void, x_pt: f32, y_pt: f32);
        fn renderer_touch_ended(ptr: *mut c_void, x_pt: f32, y_pt: f32);

        // Notification polling (called from Swift each tick)
        fn renderer_has_pending_notification(ptr: *mut c_void) -> bool;
        fn renderer_notification_title(ptr: *mut c_void) -> String;
        fn renderer_consume_notification_body(ptr: *mut c_void) -> String;

        // iOS keyboard support
        fn renderer_wants_keyboard(ptr: *mut c_void) -> bool;
        fn renderer_insert_text(ptr: *mut c_void, text: String);
        fn renderer_delete_backward(ptr: *mut c_void);

        // iPad hardware keyboard support
        fn renderer_key_event(ptr: *mut c_void, key_code: i32, modifier_flags: i32, pressed: bool);

        // iPad trackpad / mouse scroll support
        fn renderer_scroll(ptr: *mut c_void, dx: f32, dy: f32);

        // iPad trackpad hover (pointer moved without click)
        fn renderer_pointer_moved(ptr: *mut c_void, x_pt: f32, y_pt: f32);

        // Save directory & share sheet
        fn renderer_set_save_directory(ptr: *mut c_void, path: String);
        fn renderer_has_pending_share(ptr: *mut c_void) -> bool;
        fn renderer_consume_pending_share_path(ptr: *mut c_void) -> String;
    }
}

pub fn renderer_new(layer_ptr: *mut c_void, width_px: u32, height_px: u32, pixels_per_point: f32) -> *mut c_void {
    let r = Renderer::new(layer_ptr, width_px, height_px, pixels_per_point);
    Box::into_raw(Box::new(r)) as *mut c_void
}

pub fn renderer_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        unsafe { drop(Box::from_raw(ptr as *mut Renderer)) };
    }
}

pub fn renderer_resize(ptr: *mut c_void, width_px: u32, height_px: u32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.resize(width_px, height_px);
}

pub fn renderer_set_pixels_per_point(ptr: *mut c_void, pixels_per_point: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.set_pixels_per_point(pixels_per_point);
}

pub fn renderer_render(ptr: *mut c_void, time_seconds: f64) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.render(time_seconds);
}

pub fn renderer_touch_began(ptr: *mut c_void, x_pt: f32, y_pt: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.touch_began(x_pt, y_pt);
}

pub fn renderer_touch_moved(ptr: *mut c_void, x_pt: f32, y_pt: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.touch_moved(x_pt, y_pt);
}

pub fn renderer_touch_ended(ptr: *mut c_void, x_pt: f32, y_pt: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.touch_ended(x_pt, y_pt);
}

// ── Notification bridge functions ─────────────────────────────────────

pub fn renderer_has_pending_notification(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }
    unsafe { &*(ptr as *mut Renderer) }.has_pending_notification()
}

pub fn renderer_notification_title(ptr: *mut c_void) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { &*(ptr as *mut Renderer) }.notification_title()
}

/// Returns the body AND pops the notification from the queue.
pub fn renderer_consume_notification_body(ptr: *mut c_void) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { &mut *(ptr as *mut Renderer) }.consume_notification_body()
}

// ── iOS keyboard bridge functions ─────────────────────────────────────

pub fn renderer_wants_keyboard(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }
    unsafe { &*(ptr as *mut Renderer) }.wants_keyboard()
}

pub fn renderer_insert_text(ptr: *mut c_void, text: String) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.insert_text(&text);
}

pub fn renderer_delete_backward(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.delete_backward();
}

// ── iPad hardware keyboard bridge functions ───────────────────────────

pub fn renderer_key_event(ptr: *mut c_void, key_code: i32, modifier_flags: i32, pressed: bool) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.key_event(key_code, modifier_flags, pressed);
}

// ── iPad trackpad / mouse scroll bridge functions ─────────────────────

pub fn renderer_scroll(ptr: *mut c_void, dx: f32, dy: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.scroll_event(dx, dy);
}

pub fn renderer_pointer_moved(ptr: *mut c_void, x_pt: f32, y_pt: f32) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.pointer_moved(x_pt, y_pt);
}

// ── Save directory & share sheet bridge functions ─────────────────────

pub fn renderer_set_save_directory(ptr: *mut c_void, path: String) {
    if ptr.is_null() {
        return;
    }
    unsafe { &mut *(ptr as *mut Renderer) }.set_save_directory(&path);
}

pub fn renderer_has_pending_share(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }
    unsafe { &*(ptr as *mut Renderer) }.has_pending_share()
}

pub fn renderer_consume_pending_share_path(ptr: *mut c_void) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { &mut *(ptr as *mut Renderer) }.consume_pending_share_path()
}
