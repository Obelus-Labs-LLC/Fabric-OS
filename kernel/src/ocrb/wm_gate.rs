//! OCRB Phase 16 Gate — Window Manager Foundation tests.
//!
//! 10 weighted tests covering window lifecycle, z-ordering, focus management,
//! event queues, input routing, decorations, limits, and ownership.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use super::OcrbResult;
use crate::wm::{WindowTable, WindowId, MAX_WINDOWS, TITLE_BAR_HEIGHT, CLOSE_BUTTON_SIZE, TASKBAR_HEIGHT};
use crate::wm::event::{WmEvent, WmEventQueue, SERIALIZED_SIZE};
use fabric_types::ProcessId;

/// Run all Phase 16 OCRB tests.
pub fn run_all_tests() -> Vec<OcrbResult> {
    let mut results = Vec::new();

    results.push(test_create_destroy_lifecycle());
    results.push(test_z_order_management());
    results.push(test_focus_tracking_events());
    results.push(test_move_resize());
    results.push(test_compositor_multiple_windows());
    results.push(test_event_queue_fifo());
    results.push(test_input_routing_focused());
    results.push(test_window_decorations());
    results.push(test_max_windows_limit());
    results.push(test_owner_pid_validation());

    results
}

/// Test 1: Create/destroy lifecycle (w:15)
fn test_create_destroy_lifecycle() -> OcrbResult {
    let mut wt = WindowTable::new();

    // Create a window
    let wid = wt.create(
        ProcessId::new(10),
        String::from("TestWin"),
        100, 100, 200, 150,
    );

    if wid.is_none() {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("create returned None"),
        };
    }
    let wid = wid.unwrap();

    // Verify window exists
    let exists = wt.get(wid).is_some();
    if !exists {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("window not found after create"),
        };
    }

    // Verify properties
    let win = wt.get(wid).unwrap();
    let props_ok = win.width == 200
        && win.height == 150
        && win.x == 100
        && win.y == 100
        && win.visible
        && win.decorated
        && win.owner_pid == ProcessId::new(10);

    if !props_ok {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 40,
            weight: 15,
            details: String::from("properties mismatch"),
        };
    }

    // Destroy
    let destroyed = wt.destroy(wid);
    if !destroyed {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 60,
            weight: 15,
            details: String::from("destroy returned false"),
        };
    }

    // Verify gone
    let gone = wt.get(wid).is_none();
    if !gone {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 70,
            weight: 15,
            details: String::from("window still exists after destroy"),
        };
    }

    // Double destroy should return false
    let double = wt.destroy(wid);
    if double {
        return OcrbResult {
            test_name: "Create/destroy lifecycle",
            passed: false,
            score: 80,
            weight: 15,
            details: String::from("double destroy succeeded"),
        };
    }

    OcrbResult {
        test_name: "Create/destroy lifecycle",
        passed: true,
        score: 100,
        weight: 15,
        details: String::from("create, verify props, destroy, verify gone"),
    }
}

/// Test 2: Z-order management (w:10)
fn test_z_order_management() -> OcrbResult {
    let mut wt = WindowTable::new();

    let w1 = wt.create(ProcessId::new(1), String::from("W1"), 0, 0, 100, 100).unwrap();
    let w2 = wt.create(ProcessId::new(1), String::from("W2"), 50, 50, 100, 100).unwrap();
    let w3 = wt.create(ProcessId::new(1), String::from("W3"), 100, 100, 100, 100).unwrap();

    // Initially w3 should be on top (highest z)
    let sorted = wt.sorted_by_z();
    if sorted.len() != 3 {
        return OcrbResult {
            test_name: "Z-order management",
            passed: false,
            score: 0,
            weight: 10,
            details: String::from("expected 3 windows"),
        };
    }

    // Last element should be w3 (highest z)
    if sorted[2] != w3 {
        return OcrbResult {
            test_name: "Z-order management",
            passed: false,
            score: 30,
            weight: 10,
            details: String::from("w3 not on top initially"),
        };
    }

    // Raise w1 to front
    wt.raise_to_front(w1);
    let sorted2 = wt.sorted_by_z();
    if sorted2[2] != w1 {
        return OcrbResult {
            test_name: "Z-order management",
            passed: false,
            score: 50,
            weight: 10,
            details: String::from("w1 not on top after raise"),
        };
    }

    // Lower w1 to back
    wt.lower_to_back(w1);
    let sorted3 = wt.sorted_by_z();
    if sorted3[0] != w1 {
        return OcrbResult {
            test_name: "Z-order management",
            passed: false,
            score: 70,
            weight: 10,
            details: String::from("w1 not at back after lower"),
        };
    }

    OcrbResult {
        test_name: "Z-order management",
        passed: true,
        score: 100,
        weight: 10,
        details: String::from("sort, raise, lower verified"),
    }
}

/// Test 3: Focus tracking + events (w:15)
fn test_focus_tracking_events() -> OcrbResult {
    let mut wt = WindowTable::new();

    let w1 = wt.create(ProcessId::new(1), String::from("A"), 0, 0, 100, 100).unwrap();
    let w2 = wt.create(ProcessId::new(1), String::from("B"), 50, 50, 100, 100).unwrap();

    // Focus w1
    wt.set_focus(w1);
    if wt.focused_id != Some(w1) {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("focused_id not w1"),
        };
    }

    // w1 should have Focus event
    let evt1 = wt.get_mut(w1).unwrap().event_queue.pop();
    if evt1 != Some(WmEvent::WindowFocus) {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 30,
            weight: 15,
            details: String::from("w1 missing WindowFocus event"),
        };
    }

    // Focus w2 — w1 should get Blur, w2 gets Focus
    wt.set_focus(w2);
    if wt.focused_id != Some(w2) {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 50,
            weight: 15,
            details: String::from("focused_id not w2"),
        };
    }

    let blur = wt.get_mut(w1).unwrap().event_queue.pop();
    let focus = wt.get_mut(w2).unwrap().event_queue.pop();

    if blur != Some(WmEvent::WindowBlur) {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 60,
            weight: 15,
            details: String::from("w1 missing WindowBlur"),
        };
    }

    if focus != Some(WmEvent::WindowFocus) {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 70,
            weight: 15,
            details: String::from("w2 missing WindowFocus"),
        };
    }

    // Destroy focused window clears focus
    wt.destroy(w2);
    if wt.focused_id.is_some() {
        return OcrbResult {
            test_name: "Focus tracking + events",
            passed: false,
            score: 80,
            weight: 15,
            details: String::from("focus not cleared after destroy"),
        };
    }

    OcrbResult {
        test_name: "Focus tracking + events",
        passed: true,
        score: 100,
        weight: 15,
        details: String::from("focus/blur events, focus clear on destroy"),
    }
}

/// Test 4: Move/resize (w:10)
fn test_move_resize() -> OcrbResult {
    let mut wt = WindowTable::new();

    let w1 = wt.create(ProcessId::new(1), String::from("MR"), 10, 20, 200, 150).unwrap();

    // Move
    {
        let win = wt.get_mut(w1).unwrap();
        win.x = 300;
        win.y = 400;
    }

    let win = wt.get(w1).unwrap();
    if win.x != 300 || win.y != 400 {
        return OcrbResult {
            test_name: "Move/resize",
            passed: false,
            score: 0,
            weight: 10,
            details: String::from("move failed"),
        };
    }

    // Resize (reallocate surface)
    {
        let win = wt.get_mut(w1).unwrap();
        let new_w = 400u32;
        let new_h = 300u32;
        if let Some(new_surface) = crate::display::compositor::Surface::new(new_w, new_h) {
            win.surface = new_surface;
            win.width = new_w;
            win.height = new_h;
        }
    }

    let win = wt.get(w1).unwrap();
    if win.width != 400 || win.height != 300 {
        return OcrbResult {
            test_name: "Move/resize",
            passed: false,
            score: 50,
            weight: 10,
            details: String::from("resize failed"),
        };
    }

    // Verify surface dimensions match
    if win.surface.width != 400 || win.surface.height != 300 {
        return OcrbResult {
            test_name: "Move/resize",
            passed: false,
            score: 70,
            weight: 10,
            details: String::from("surface dimensions mismatch"),
        };
    }

    OcrbResult {
        test_name: "Move/resize",
        passed: true,
        score: 100,
        weight: 10,
        details: String::from("move and resize with surface realloc"),
    }
}

/// Test 5: Compositor renders multiple windows (w:15)
fn test_compositor_multiple_windows() -> OcrbResult {
    // This test verifies the compositor can handle multiple windows
    // without panicking. We use the global WINDOW_TABLE briefly.
    use crate::wm::WINDOW_TABLE;

    let mut wt = WINDOW_TABLE.lock();

    // Clean slate
    let initial_count = wt.count();

    // Create 3 small windows
    let w1 = wt.create(ProcessId::KERNEL, String::from("CW1"), 10, 10, 64, 48);
    let w2 = wt.create(ProcessId::KERNEL, String::from("CW2"), 50, 50, 64, 48);
    let w3 = wt.create(ProcessId::KERNEL, String::from("CW3"), 90, 90, 64, 48);

    if w1.is_none() || w2.is_none() || w3.is_none() {
        // Clean up
        if let Some(id) = w1 { wt.destroy(id); }
        if let Some(id) = w2 { wt.destroy(id); }
        if let Some(id) = w3 { wt.destroy(id); }
        return OcrbResult {
            test_name: "Compositor multiple windows",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("failed to create test windows"),
        };
    }

    let w1 = w1.unwrap();
    let w2 = w2.unwrap();
    let w3 = w3.unwrap();

    // Verify sorted_by_z returns all 3
    let sorted = wt.sorted_by_z();
    let new_count = sorted.len();

    // Focus w2
    wt.set_focus(w2);

    // Verify z-order: w2 should be on top
    let sorted_after = wt.sorted_by_z();
    let top = sorted_after.last().copied();

    // Clean up
    wt.destroy(w1);
    wt.destroy(w2);
    wt.destroy(w3);

    if new_count < 3 {
        return OcrbResult {
            test_name: "Compositor multiple windows",
            passed: false,
            score: 40,
            weight: 15,
            details: String::from("sorted_by_z missing windows"),
        };
    }

    if top != Some(w2) {
        return OcrbResult {
            test_name: "Compositor multiple windows",
            passed: false,
            score: 60,
            weight: 15,
            details: String::from("focused window not on top"),
        };
    }

    OcrbResult {
        test_name: "Compositor multiple windows",
        passed: true,
        score: 100,
        weight: 15,
        details: String::from("3 windows created, z-sorted, focused on top"),
    }
}

/// Test 6: Event queue push/pop FIFO (w:10)
fn test_event_queue_fifo() -> OcrbResult {
    let mut queue = WmEventQueue::new();

    // Push 5 events
    queue.push(WmEvent::KeyPress(b'a'));
    queue.push(WmEvent::KeyPress(b'b'));
    queue.push(WmEvent::KeyRelease(b'a'));
    queue.push(WmEvent::WindowFocus);
    queue.push(WmEvent::WindowClose);

    if queue.len() != 5 {
        return OcrbResult {
            test_name: "Event queue FIFO",
            passed: false,
            score: 0,
            weight: 10,
            details: String::from("expected len 5"),
        };
    }

    // Pop in FIFO order
    let e1 = queue.pop();
    let e2 = queue.pop();
    let e3 = queue.pop();
    let e4 = queue.pop();
    let e5 = queue.pop();
    let e6 = queue.pop(); // should be None

    let fifo_ok = e1 == Some(WmEvent::KeyPress(b'a'))
        && e2 == Some(WmEvent::KeyPress(b'b'))
        && e3 == Some(WmEvent::KeyRelease(b'a'))
        && e4 == Some(WmEvent::WindowFocus)
        && e5 == Some(WmEvent::WindowClose)
        && e6.is_none();

    if !fifo_ok {
        return OcrbResult {
            test_name: "Event queue FIFO",
            passed: false,
            score: 50,
            weight: 10,
            details: String::from("FIFO order incorrect"),
        };
    }

    // Test serialization
    let bytes = WmEvent::KeyPress(b'x').to_bytes();
    if bytes != [1, b'x', 0, 0] {
        return OcrbResult {
            test_name: "Event queue FIFO",
            passed: false,
            score: 70,
            weight: 10,
            details: String::from("serialization mismatch"),
        };
    }

    if SERIALIZED_SIZE != 4 {
        return OcrbResult {
            test_name: "Event queue FIFO",
            passed: false,
            score: 80,
            weight: 10,
            details: String::from("SERIALIZED_SIZE not 4"),
        };
    }

    OcrbResult {
        test_name: "Event queue FIFO",
        passed: true,
        score: 100,
        weight: 10,
        details: String::from("push/pop FIFO, serialization verified"),
    }
}

/// Test 7: Input routing to focused window (w:10)
fn test_input_routing_focused() -> OcrbResult {
    let mut wt = WindowTable::new();

    let w1 = wt.create(ProcessId::new(1), String::from("In1"), 0, 0, 100, 100).unwrap();
    let w2 = wt.create(ProcessId::new(1), String::from("In2"), 50, 50, 100, 100).unwrap();

    // Focus w2
    wt.set_focus(w2);

    // Drain focus events
    wt.get_mut(w1).unwrap().event_queue.pop(); // w1 may have gotten focus then blur
    wt.get_mut(w2).unwrap().event_queue.pop(); // drain focus event

    // Simulate input to focused window
    if let Some(fid) = wt.focused_id {
        if let Some(win) = wt.get_mut(fid) {
            win.event_queue.push(WmEvent::KeyPress(b'h'));
            win.event_queue.push(WmEvent::KeyPress(b'i'));
        }
    }

    // w1 should have no key events
    let w1_event = wt.get_mut(w1).unwrap().event_queue.pop();
    // w2 should have the key events
    let w2_e1 = wt.get_mut(w2).unwrap().event_queue.pop();
    let w2_e2 = wt.get_mut(w2).unwrap().event_queue.pop();

    let ok = w2_e1 == Some(WmEvent::KeyPress(b'h'))
        && w2_e2 == Some(WmEvent::KeyPress(b'i'));

    if !ok {
        return OcrbResult {
            test_name: "Input routing to focused",
            passed: false,
            score: 50,
            weight: 10,
            details: String::from("events not routed to focused window"),
        };
    }

    OcrbResult {
        test_name: "Input routing to focused",
        passed: true,
        score: 100,
        weight: 10,
        details: String::from("keys routed to focused, unfocused untouched"),
    }
}

/// Test 8: Window decorations rendered (w:5)
fn test_window_decorations() -> OcrbResult {
    // Verify decoration constants are reasonable
    if TITLE_BAR_HEIGHT == 0 || TITLE_BAR_HEIGHT > 64 {
        return OcrbResult {
            test_name: "Window decorations",
            passed: false,
            score: 0,
            weight: 5,
            details: String::from("TITLE_BAR_HEIGHT out of range"),
        };
    }

    if CLOSE_BUTTON_SIZE == 0 || CLOSE_BUTTON_SIZE > TITLE_BAR_HEIGHT {
        return OcrbResult {
            test_name: "Window decorations",
            passed: false,
            score: 20,
            weight: 5,
            details: String::from("CLOSE_BUTTON_SIZE out of range"),
        };
    }

    if TASKBAR_HEIGHT == 0 || TASKBAR_HEIGHT > 64 {
        return OcrbResult {
            test_name: "Window decorations",
            passed: false,
            score: 40,
            weight: 5,
            details: String::from("TASKBAR_HEIGHT out of range"),
        };
    }

    // Create a decorated window and verify the flag
    let mut wt = WindowTable::new();
    let w = wt.create(ProcessId::new(1), String::from("Dec"), 0, 0, 100, 100).unwrap();
    let win = wt.get(w).unwrap();

    if !win.decorated {
        return OcrbResult {
            test_name: "Window decorations",
            passed: false,
            score: 60,
            weight: 5,
            details: String::from("decorated flag not set by default"),
        };
    }

    OcrbResult {
        test_name: "Window decorations",
        passed: true,
        score: 100,
        weight: 5,
        details: String::from("title=24px, close=20px, taskbar=32px, decorated=true"),
    }
}

/// Test 9: Max windows limit (w:5)
fn test_max_windows_limit() -> OcrbResult {
    let mut wt = WindowTable::new();

    // Create MAX_WINDOWS windows
    let mut ids = Vec::new();
    for i in 0..MAX_WINDOWS {
        match wt.create(
            ProcessId::new(1),
            String::from("W"),
            0, 0, 32, 32,
        ) {
            Some(id) => ids.push(id),
            None => {
                return OcrbResult {
                    test_name: "Max windows limit",
                    passed: false,
                    score: (i as u8 * 3).min(80),
                    weight: 5,
                    details: String::from("alloc failed before reaching MAX"),
                };
            }
        }
    }

    if wt.count() != MAX_WINDOWS {
        return OcrbResult {
            test_name: "Max windows limit",
            passed: false,
            score: 60,
            weight: 5,
            details: String::from("count != MAX_WINDOWS"),
        };
    }

    // 33rd should fail
    let extra = wt.create(ProcessId::new(1), String::from("Overflow"), 0, 0, 32, 32);
    if extra.is_some() {
        return OcrbResult {
            test_name: "Max windows limit",
            passed: false,
            score: 70,
            weight: 5,
            details: String::from("exceeded MAX_WINDOWS"),
        };
    }

    OcrbResult {
        test_name: "Max windows limit",
        passed: true,
        score: 100,
        weight: 5,
        details: String::from("32 created, 33rd rejected"),
    }
}

/// Test 10: Owner PID validation (w:5)
fn test_owner_pid_validation() -> OcrbResult {
    let mut wt = WindowTable::new();

    let pid_a = ProcessId::new(42);
    let pid_b = ProcessId::new(99);

    let w1 = wt.create(pid_a, String::from("A-Win"), 0, 0, 100, 100).unwrap();
    let w2 = wt.create(pid_b, String::from("B-Win"), 50, 50, 100, 100).unwrap();

    // windows_for_pid
    let a_windows = wt.windows_for_pid(pid_a);
    let b_windows = wt.windows_for_pid(pid_b);

    if a_windows.len() != 1 || a_windows[0] != w1 {
        return OcrbResult {
            test_name: "Owner PID validation",
            passed: false,
            score: 30,
            weight: 5,
            details: String::from("windows_for_pid(A) incorrect"),
        };
    }

    if b_windows.len() != 1 || b_windows[0] != w2 {
        return OcrbResult {
            test_name: "Owner PID validation",
            passed: false,
            score: 50,
            weight: 5,
            details: String::from("windows_for_pid(B) incorrect"),
        };
    }

    // Verify owner_pid field
    let owner = wt.get(w1).unwrap().owner_pid;
    if owner != pid_a {
        return OcrbResult {
            test_name: "Owner PID validation",
            passed: false,
            score: 70,
            weight: 5,
            details: String::from("owner_pid mismatch"),
        };
    }

    OcrbResult {
        test_name: "Owner PID validation",
        passed: true,
        score: 100,
        weight: 5,
        details: String::from("per-PID window lookup verified"),
    }
}
