#![allow(dead_code)]

// ============================================================
// PS/2 mouse packet decoder
// ============================================================
//
// Standard 3-byte packet:
//
//     byte 0  bit0 left, bit1 right, bit2 middle
//             bit3 always 1 (used for resynchronization)
//             bit4 X sign, bit5 Y sign
//             bit6 X overflow, bit7 Y overflow
//     byte 1  relative X (two's complement)
//     byte 2  relative Y (two's complement, positive = up)
//
// IntelliMouse devices (device id 0x03, see ps2::mod) append:
//
//     byte 3  low nibble = signed wheel delta, high nibble = buttons 4/5
//
// Packets are decoded into the shared InputEvent queue; the compositor
// consumes MouseMove/MouseButton* and never sees raw bytes.
// ============================================================

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use spin::Mutex;

static PACKET: Mutex<[u8; 4]> = Mutex::new([0; 4]);
static INDEX: AtomicU8 = AtomicU8::new(0);

/// Largest packet the decoder understands (IntelliMouse with wheel).
const PACKET_SIZE_LIMIT: usize = 4;

/// First complete packet of the session, for the one-shot diagnostic below.
static FIRST_PACKET: Mutex<Option<([u8; 4], usize)>> = Mutex::new(None);
static FIRST_PACKET_REPORTED: AtomicBool = AtomicBool::new(false);

const SYNC_BIT: u8 = 0x08;
const BUTTON_MASK: u8 = 0x07;

pub fn handle_byte(byte: u8) {
    let packet_size = super::mouse_packet_size();

    if packet_size == 0 || packet_size > PACKET_SIZE_LIMIT {
        return;
    }

    let index = INDEX.load(Ordering::Acquire) as usize;

    // The first byte of every packet has bit 3 set. A byte without it means
    // the stream is out of step, so drop it and resynchronize on the next
    // packet boundary.
    if index == 0 && byte & SYNC_BIT == 0 {
        return;
    }

    {
        let mut packet = PACKET.lock();
        packet[index] = byte;
    }

    let next = index + 1;

    if next < packet_size {
        INDEX.store(next as u8, Ordering::Release);
        return;
    }

    INDEX.store(0, Ordering::Release);

    let packet = *PACKET.lock();

    process(&packet, packet_size);
}

fn process(packet: &[u8; 4], packet_size: usize) {
    // Keep the first packet for normal-context reporting. Decoding continues
    // even when the diagnostic is still pending.
    if !FIRST_PACKET_REPORTED.load(Ordering::Acquire) {
        let mut slot = FIRST_PACKET.lock();

        if slot.is_none() {
            *slot = Some((*packet, packet_size));
        }
    }

    let flags = packet[0];

    let dx = packet[1] as i8 as i32;

    // PS/2 reports positive Y as "up"; the framebuffer grows downwards.
    let dy = -(packet[2] as i8 as i32);

    if dx != 0 || dy != 0 {
        crate::input::move_mouse(dx, dy);
    }

    crate::input::set_mouse_buttons(flags & BUTTON_MASK);

    if packet_size >= 4 {
        let wheel = wheel_delta(packet[3]);

        if wheel != 0 {
            crate::input::mouse_wheel(wheel);
        }
    }
}

/// Consumes the first decoded packet, once, for normal-context reporting.
pub fn take_first_packet() -> Option<([u8; 4], usize)> {
    let packet = FIRST_PACKET.lock().take();

    if packet.is_some() {
        FIRST_PACKET_REPORTED.store(true, Ordering::Release);
    }

    packet
}

/// The wheel lives in the low nibble of the fourth byte as a 4-bit signed
/// value.
fn wheel_delta(byte: u8) -> i16 {
    let nibble = byte & 0x0F;

    if nibble & 0x08 != 0 {
        nibble as i16 - 16
    } else {
        nibble as i16
    }
}
