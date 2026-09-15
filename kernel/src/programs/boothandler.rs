#![no_std]

use bootloader_api::BootInfo;
use crate::{cpu, graphics, input, interrupts, memory, serial, wasm, wm};
use crate::graphics::Color;

pub fn run_boot_sequence(boot_info: &'static mut BootInfo) {
    // ========================================================
    // Memory
    // ========================================================

    memory::init(&*boot_info);

    // ========================================================
    // Serial
    // ========================================================

    serial::init();
    serial::write_str("Rusty kernel started\n");

    // ========================================================
    // CPU / GDT / TSS
    // ========================================================

    serial::write_str("Initializing GDT/TSS\n");
    let _selectors = cpu::gdt::init();
    serial::write_str("GDT + TSS initialized\n");

    // ========================================================
    // IDT
    // ========================================================

    serial::write_str("Initializing IDT\n");
    interrupts::init();
    serial::write_str("IDT initialized\n");

    // Initialize SSE for floating-point operations
    serial::write_str("Initializing SSE\n");
    cpu::enable_sse();
    serial::write_str("SSE initialized\n");

    // ========================================================
    // Graphics
    // ========================================================

    serial::write_str("Initializing Graphics\n");



    let Some(framebuffer) = boot_info.framebuffer.as_mut() else {
        serial::write_str("ERROR: framebuffer unavailable\n");
        return;
    };

    graphics::init(framebuffer);

    serial::write_str("Graphics initialized\n");

    // ========================================================
    // Window Manager
    // ========================================================

    serial::write_str("Initializing Window Manager\n");

    let info = framebuffer.info();
    let fb_ptr = framebuffer.buffer_mut().as_mut_ptr() as *mut u32;

    wm::init(info.width, info.height, fb_ptr);

    serial::write_str("Window Manager initialized\n");

    // ========================================================
    // System Boot Screen (Compositor Controlled)
    // ========================================================

    let win_width = 420;
    let win_height = 240;
    let win_x = (info.width as i32 - win_width as i32) / 2;
    let win_y = (info.height as i32 - win_height as i32) / 2;

    let boot_win_id = if let Some(ref mut wm) = *wm::WM.lock() {
        let id = wm.create_window(win_x, win_y, win_width, win_height);

        // Draw boot screen title header bar inside boot window
        if let Some(win) = wm.windows.get_mut(&id) {
            win.fill_rect(0, 0, win_width, 32, 0xFF313244); // Header bar
            win.fill_rect(0, 32, win_width, win_height - 32, 0xFF1E1E2E); // Inner body
        }

        wm.draw();
        Some(id)
    } else {
        None
    };

    // State array to keep past text messages alive after wm.draw() clears the frame
    let mut messages: [&str; 4] = ["", "", "", ""];

    // Helper closure to update status lines, progress bars, and text
    let mut update_boot_status =
        |step: usize,
         color_argb: u32,
         task_msg: &'static str| {
            messages[step] = task_msg;

            let Some(id) = boot_win_id else {
                return;
            };

            let Some(ref mut wm) = *wm::WM.lock() else {
                return;
            };

            if let Some(win) = wm.windows.get_mut(&id) {
                // ====================================================
                // Status indicator
                // ====================================================

                let y_pos = 50 + (step * 45);

                win.fill_rect(
                    20,
                    y_pos,
                    14,
                    14,
                    color_argb,
                );

                // ====================================================
                // Clear the entire status row before redrawing text
                // ====================================================

                win.fill_rect(
                    45,
                    y_pos,
                    350,
                    20,
                    0xFF1E1E2E,
                );

                // ====================================================
                // Progress bar track
                // ====================================================

                win.fill_rect(
                    45,
                    y_pos + 22,
                    340,
                    4,
                    0xFF45475A,
                );

                // ====================================================
                // Progress bar
                // ====================================================

                let progress_width = 85 + (step * 85);

                win.fill_rect(
                    45,
                    y_pos + 22,
                    progress_width,
                    4,
                    color_argb,
                );

                // ====================================================
                // Draw all status text
                // ====================================================

                for (i, &msg) in messages.iter().enumerate() {
                    if msg.is_empty() {
                        continue;
                    }

                    let text_y = 50 + (i * 45);

                    win.draw_string(
                        45,
                        text_y,
                        msg,
                        Color::WHITE,
                        1,
                    );
                }
            }

            // ========================================================
            // Composite the window to the framebuffer
            // ========================================================

            wm.draw();
        };

    // Stage 1: Core Systems OK
    update_boot_status(0, 0xFFA6E3A1, "Task 1/4: Core Systems (GDT/IDT/Mem) OK");

    // ========================================================
    // PCI / xHCI
    // ========================================================

    let Some(bar0) = input::usb::find_xhci_bar0() else {
        update_boot_status(1, 0xFFF38BA8, "Task 2/4: ERROR - xHCI controller not found!");
        serial::write_str("ERROR: xHCI controller not found\n");
        return;
    };

    // Stage 2: xHCI Found
    update_boot_status(1, 0xFFA6E3A1, "Task 2/4: xHCI Controller Initialized");
    serial::write_str("xHCI found\n");

    // ========================================================
    // USB
    // ========================================================

    input::init(bar0);

    if input::has_keyboard() {
        // Stage 3: USB & Keyboard Detected
        update_boot_status(2, 0xFF89B4FA, "Task 3/4: USB Keyboard Detected & Ready");
        serial::write_str("USB keyboard detected\n");
    } else {
        update_boot_status(2, 0xFFF9E2AF, "Task 3/4: Warning - USB Keyboard not found");
        serial::write_str("USB keyboard not found\n");
    }

    // ========================================================
    // WASM runtime
    // ========================================================

    serial::write_str("finishing boot sequence...\n");
    update_boot_status(3, 0xFFCBA6F7, "Task 4/4: Finalizing...");
    //
    // wasm::run_demo();
    //
    update_boot_status(3, 0xFFA6E3A1, "Task 4/4: Boot Sequence Complete!");
    serial::write_str("Boot sequence finished successfully\n");

    crate::delay::delay_seconds(1);

    // Destroy boot screen window
    if let Some(id) = boot_win_id {
        if let Some(ref mut wm) = *wm::WM.lock() {
            wm.destroy_window(id);
            wm.draw();
        }
    }


}