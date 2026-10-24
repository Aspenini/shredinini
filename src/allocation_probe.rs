//! Counts allocations on only the current test thread, so parallel tests cannot
//! contaminate real-time checks. The production executable uses Rust's allocator.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static ENABLED:Cell<bool>=const {Cell::new(false)};
    static COUNT:Cell<usize>=const {Cell::new(0)};
}
struct Probe;
#[global_allocator]
static ALLOCATOR: Probe = Probe;
fn record() {
    let _ = ENABLED.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }
    });
}
unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(p, layout, new_size) }
    }
}
pub fn count(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ENABLED.with(|e| e.set(true));
    struct Disable;
    impl Drop for Disable {
        fn drop(&mut self) {
            ENABLED.with(|e| e.set(false));
        }
    }
    let guard = Disable;
    f();
    drop(guard);
    COUNT.with(Cell::get)
}
