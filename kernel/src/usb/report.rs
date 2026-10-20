// ============================================================
// HID report descriptor parser
// ============================================================
//
// Non-boot HID keyboards, mice and tablets describe their reports with a
// report descriptor instead of the fixed boot layout. This module parses the
// short-item format (HID 1.11 section 6.2.2) and extracts the fields needed
// for input:
//
//     keyboard: modifier bitmap (usage page 0x07, 0xE0-0xE7) and key array
//     mouse:    buttons (page 0x09), X/Y (page 0x01, 0x30/0x31) and wheel
//
// Report IDs are supported: fields are grouped per ID and offsets are
// relative to the bytes after the ID prefix.
// ============================================================

use alloc::vec::Vec;

// ============================================================
// Item constants
// ============================================================

const ITEM_TYPE_MAIN: u8 = 0;
const ITEM_TYPE_GLOBAL: u8 = 1;
const ITEM_TYPE_LOCAL: u8 = 2;

const MAIN_INPUT: u8 = 8;
const MAIN_COLLECTION: u8 = 10;
const MAIN_END_COLLECTION: u8 = 12;

const GLOBAL_USAGE_PAGE: u8 = 0;
const GLOBAL_LOGICAL_MIN: u8 = 1;
const GLOBAL_LOGICAL_MAX: u8 = 2;
const GLOBAL_REPORT_SIZE: u8 = 7;
const GLOBAL_REPORT_ID: u8 = 8;
const GLOBAL_REPORT_COUNT: u8 = 9;
const GLOBAL_PUSH: u8 = 10;
const GLOBAL_POP: u8 = 11;

const LOCAL_USAGE: u8 = 0;
const LOCAL_USAGE_MIN: u8 = 1;
const LOCAL_USAGE_MAX: u8 = 2;

const USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
const USAGE_PAGE_KEYBOARD: u16 = 0x07;
const USAGE_PAGE_BUTTON: u16 = 0x09;

const USAGE_MOUSE: u32 = 0x02;
const USAGE_KEYBOARD: u32 = 0x06;
const USAGE_X: u32 = 0x30;
const USAGE_Y: u32 = 0x31;
const USAGE_WHEEL: u32 = 0x38;

// ============================================================
// Field layout
// ============================================================

#[derive(Clone, Copy)]
pub struct Field {
    /// Bit offset from the start of the report data (after any report ID).
    pub offset: usize,
    pub size: usize,
    pub count: usize,
    pub usage_page: u16,
    pub usage: u32,
    pub logical_min: i32,
    pub logical_max: i32,
    pub relative: bool,
}

pub struct KeyboardLayout {
    /// Variable bitmap for usages 0xE0-0xE7, usually 8 bits.
    pub modifiers: Option<Field>,
    /// Array of key usages.
    pub keys: Field,
}

pub struct MouseLayout {
    /// One field per button bit, indexed by usage.
    pub buttons: Vec<Field>,
    pub x: Field,
    pub y: Field,
    pub wheel: Option<Field>,
    pub absolute: bool,
}

pub enum Layout {
    Keyboard(KeyboardLayout),
    Mouse(MouseLayout),
}

pub struct ReportLayout {
    pub report_id: Option<u8>,
    pub bits: usize,
    pub layout: Layout,
}

impl ReportLayout {
    pub fn kind_name(&self) -> &'static str {
        match self.layout {
            Layout::Keyboard(_) => "keyboard",
            Layout::Mouse(_) => "mouse",
        }
    }

    pub fn byte_len(&self) -> usize {
        let bits = self.bits + if self.report_id.is_some() { 8 } else { 0 };

        (bits + 7) / 8
    }

    /// Decode one input report.
    ///
    /// Returns the first newly pressed keyboard usage, for the one-shot
    /// diagnostic in the USB subsystem.
    pub fn decode(&self, report: &[u8], state: &mut DecodeState) -> Option<u8> {
        let data = match self.report_id {
            Some(id) => {
                if report.first().copied() != Some(id) {
                    return None;
                }

                &report[1..]
            }
            None => report,
        };

        match &self.layout {
            Layout::Keyboard(keyboard) => decode_keyboard(keyboard, data, state),
            Layout::Mouse(mouse) => {
                decode_mouse(mouse, data, state);
                None
            }
        }
    }
}

pub struct DecodeState {
    pub keys: [u8; 6],
    pub modifiers: u8,
    pub last_x: i32,
    pub last_y: i32,
    pub have_position: bool,
}

impl DecodeState {
    pub const fn new() -> Self {
        Self {
            keys: [0; 6],
            modifiers: 0,
            last_x: 0,
            last_y: 0,
            have_position: false,
        }
    }
}

// ============================================================
// Parsing
// ============================================================

#[derive(Clone, Copy)]
struct Globals {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: usize,
    report_count: usize,
    report_id: Option<u8>,
}

impl Globals {
    const fn new() -> Self {
        Self {
            usage_page: 0,
            logical_min: 0,
            logical_max: 0,
            report_size: 0,
            report_count: 0,
            report_id: None,
        }
    }
}

#[derive(Default)]
struct Locals {
    usages: Vec<u32>,
    usage_min: Option<u32>,
    usage_max: Option<u32>,
    usage_min_page: u16,
    usage_max_page: u16,
}

struct Candidate {
    id: Option<u8>,
    bits: usize,
    app_page: u16,
    app_usage: u32,
    modifiers: Option<Field>,
    keys: Option<Field>,
    buttons: Vec<Field>,
    x: Option<Field>,
    y: Option<Field>,
    wheel: Option<Field>,
    absolute: bool,
}

impl Candidate {
    fn new(id: Option<u8>) -> Self {
        Self {
            id,
            bits: 0,
            app_page: 0,
            app_usage: 0,
            modifiers: None,
            keys: None,
            buttons: Vec::new(),
            x: None,
            y: None,
            wheel: None,
            absolute: false,
        }
    }
}

pub fn parse(descriptor: &[u8]) -> Option<ReportLayout> {
    let mut globals = Globals::new();
    let mut globals_stack = Vec::new();
    let mut locals = Locals::default();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut collection_stack: Vec<(u16, u32)> = Vec::new();

    let mut index = 0usize;

    while index < descriptor.len() {
        let prefix = descriptor[index];
        index += 1;

        // Long item: 0xFE, data size, tag, data.
        if prefix == 0xFE {
            if index + 1 >= descriptor.len() {
                break;
            }

            let size = descriptor[index] as usize;
            index += 2;

            index = index.saturating_add(size);
            continue;
        }

        let size_code = (prefix & 0x03) as usize;
        let size = match size_code {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };

        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;

        if index + size > descriptor.len() {
            break;
        }

        let mut value = 0u32;

        for byte in 0..size {
            value |= (descriptor[index + byte] as u32) << (8 * byte);
        }

        index += size;

        match item_type {
            ITEM_TYPE_GLOBAL => match tag {
                GLOBAL_USAGE_PAGE => globals.usage_page = value as u16,

                GLOBAL_LOGICAL_MIN => globals.logical_min = sign_extend(value, size),

                GLOBAL_LOGICAL_MAX => globals.logical_max = sign_extend(value, size),

                GLOBAL_REPORT_SIZE => globals.report_size = value as usize,

                GLOBAL_REPORT_COUNT => globals.report_count = value as usize,

                GLOBAL_REPORT_ID => globals.report_id = Some(value as u8),

                GLOBAL_PUSH => globals_stack.push(globals),

                GLOBAL_POP => {
                    if let Some(previous) = globals_stack.pop() {
                        globals = previous;
                    }
                }

                _ => {}
            },

            ITEM_TYPE_LOCAL => match tag {
                LOCAL_USAGE => {
                    if size == 4 {
                        // Extended usage: high half is the usage page.
                        locals.usages.push(value & 0xFFFF);
                    } else {
                        locals.usages.push(value);
                    }
                }

                LOCAL_USAGE_MIN => {
                    locals.usage_min = Some(value & 0xFFFF);

                    if size == 4 {
                        locals.usage_min_page = (value >> 16) as u16;
                    }
                }

                LOCAL_USAGE_MAX => {
                    locals.usage_max = Some(value & 0xFFFF);

                    if size == 4 {
                        locals.usage_max_page = (value >> 16) as u16;
                    }
                }

                _ => {}
            },

            ITEM_TYPE_MAIN => {
                let candidate_index = candidate_for(&mut candidates, globals.report_id);
                let candidate = &mut candidates[candidate_index];

                match tag {
                    MAIN_COLLECTION => {
                        let usage = locals.usages.first().copied().unwrap_or(0);
                        let page = if locals.usages.is_empty() {
                            globals.usage_page
                        } else {
                            globals.usage_page
                        };

                        // Collection type is in the low two data bits.
                        if value & 0x03 == 1 {
                            candidate.app_page = page;
                            candidate.app_usage = usage;
                        }

                        collection_stack.push((page, usage));
                    }

                    MAIN_END_COLLECTION => {
                        collection_stack.pop();
                    }

                    MAIN_INPUT => {
                        let total = globals.report_size * globals.report_count;
                        let offset = candidate.bits;

                        // Bit 0 is Constant; bit 1 is Variable; bit 2 is
                        // Relative. Constant fields advance the offset but
                        // produce no input.
                        let constant = value & 0x01 != 0;
                        let variable = value & 0x02 != 0;
                        let relative = value & 0x04 != 0;

                        if !constant && globals.report_size > 0 {
                            if variable {
                                for item in 0..globals.report_count {
                                    let usage = usage_at(&locals, item);
                                    let field = Field {
                                        offset: offset + item * globals.report_size,
                                        size: globals.report_size,
                                        count: 1,
                                        usage_page: globals.usage_page,
                                        usage,
                                        logical_min: globals.logical_min,
                                        logical_max: globals.logical_max,
                                        relative,
                                    };

                                    classify(candidate, field);
                                }
                            } else if globals.usage_page == USAGE_PAGE_KEYBOARD
                                && candidate.keys.is_none()
                            {
                                candidate.keys = Some(Field {
                                    offset,
                                    size: globals.report_size,
                                    count: globals.report_count,
                                    usage_page: globals.usage_page,
                                    usage: 0,
                                    logical_min: globals.logical_min,
                                    logical_max: globals.logical_max,
                                    relative,
                                });
                            }
                        }

                        candidate.bits = candidate.bits.saturating_add(total);
                    }

                    _ => {}
                }

                locals = Locals::default();
            }

            _ => {}
        }
    }

    let _ = collection_stack;

    choose_layout(&candidates)
}

fn candidate_for(candidates: &mut Vec<Candidate>, id: Option<u8>) -> usize {
    if let Some(index) = candidates.iter().position(|candidate| candidate.id == id) {
        return index;
    }

    candidates.push(Candidate::new(id));
    candidates.len() - 1
}

fn usage_at(locals: &Locals, index: usize) -> u32 {
    if let Some(usage) = locals.usages.get(index) {
        return *usage;
    }

    match (locals.usage_min, locals.usage_max) {
        (Some(min), Some(_)) => min + index as u32,
        (Some(min), None) => min + index as u32,
        _ => 0,
    }
}

fn classify(candidate: &mut Candidate, field: Field) {
    if field.usage_page == USAGE_PAGE_KEYBOARD
        && field.size == 1
        && (0xE0..=0xE7).contains(&field.usage)
    {
        if candidate.modifiers.is_none() {
            candidate.modifiers = Some(field);
        }

        return;
    }

    if field.usage_page == USAGE_PAGE_BUTTON {
        candidate.buttons.push(field);
        return;
    }

    if field.usage_page == USAGE_PAGE_GENERIC_DESKTOP {
        match field.usage {
            USAGE_X => {
                candidate.x = Some(field);
                candidate.absolute = !field.relative;
            }

            USAGE_Y => {
                candidate.y = Some(field);
                candidate.absolute = !field.relative;
            }

            USAGE_WHEEL => candidate.wheel = Some(field),

            _ => {}
        }
    }
}

fn choose_layout(candidates: &[Candidate]) -> Option<ReportLayout> {
    // Prefer the candidate whose application collection says keyboard or
    // mouse; fall back to the fields that were found.
    let mut keyboard: Option<&Candidate> = None;
    let mut mouse: Option<&Candidate> = None;

    for candidate in candidates {
        let app_keyboard = candidate.app_page == USAGE_PAGE_GENERIC_DESKTOP
            && candidate.app_usage == USAGE_KEYBOARD;

        let app_mouse = candidate.app_page == USAGE_PAGE_GENERIC_DESKTOP
            && (candidate.app_usage == USAGE_MOUSE || candidate.app_usage == 0x01);

        if app_keyboard && candidate.keys.is_some() {
            keyboard = Some(candidate);
            break;
        }

        if app_mouse && candidate.x.is_some() && candidate.y.is_some() {
            mouse = Some(candidate);
            break;
        }
    }

    if keyboard.is_none() {
        keyboard = candidates.iter().find(|candidate| {
            candidate.keys.is_some()
                && (candidate.modifiers.is_some()
                    || candidate.app_usage == USAGE_KEYBOARD
                    || candidate.app_usage == 0)
        });
    }

    if mouse.is_none() {
        mouse = candidates
            .iter()
            .find(|candidate| candidate.x.is_some() && candidate.y.is_some());
    }

    // Prefer a keyboard when both are present, unless it is an obvious
    // pointer-only descriptor.
    if let Some(candidate) = keyboard {
        if let Some(keys) = candidate.keys {
            return Some(ReportLayout {
                report_id: candidate.id,
                bits: candidate.bits,
                layout: Layout::Keyboard(KeyboardLayout {
                    modifiers: candidate.modifiers,
                    keys,
                }),
            });
        }
    }

    if let Some(candidate) = mouse {
        let x = candidate.x?;
        let y = candidate.y?;

        return Some(ReportLayout {
            report_id: candidate.id,
            bits: candidate.bits,
            layout: Layout::Mouse(MouseLayout {
                buttons: candidate.buttons.clone(),
                x,
                y,
                wheel: candidate.wheel,
                absolute: candidate.absolute,
            }),
        });
    }

    None
}

// ============================================================
// Decoding
// ============================================================

fn decode_keyboard(
    keyboard: &KeyboardLayout,
    report: &[u8],
    state: &mut DecodeState,
) -> Option<u8> {
    let mut boot = [0u8; 8];

    if let Some(modifiers) = keyboard.modifiers {
        let mut value = 0u8;

        for bit in 0..8 {
            value |= ((extract(report, modifiers.offset + bit, 1) & 1) as u8) << bit;
        }

        boot[0] = value;
    }

    let count = keyboard.keys.count.min(6);

    for index in 0..count {
        let usage = extract(
            report,
            keyboard.keys.offset + index * keyboard.keys.size,
            keyboard.keys.size,
        );

        boot[2 + index] = if usage <= 0xFF { usage as u8 } else { 0 };
    }

    crate::usb::keyboard::decode(&boot, &mut state.keys, &mut state.modifiers)
}

fn decode_mouse(mouse: &MouseLayout, report: &[u8], state: &mut DecodeState) {
    let mut buttons = 0u8;

    for field in mouse.buttons.iter() {
        if field.usage >= 1 && field.usage <= 5 && extract(report, field.offset, 1) & 1 != 0 {
            buttons |= 1 << (field.usage - 1);
        }
    }

    let x = axis(report, &mouse.x);
    let y = axis(report, &mouse.y);

    if mouse.absolute {
        // Absolute pointers (tablets) report a position in their logical
        // range. Map it onto the framebuffer and turn it into a delta; the
        // compositor cursor starts at the centre of the screen.
        let width = crate::graphics::width() as i32;
        let height = crate::graphics::height() as i32;

        let target_x = scale_axis(x, mouse.x.logical_max, width);
        let target_y = scale_axis(y, mouse.y.logical_max, height);

        let (from_x, from_y) = if state.have_position {
            (state.last_x, state.last_y)
        } else {
            (width / 2, height / 2)
        };

        let dx = target_x - from_x;
        let dy = target_y - from_y;

        if dx != 0 || dy != 0 {
            crate::input::move_mouse(dx, dy);
        }

        state.last_x = target_x;
        state.last_y = target_y;
        state.have_position = true;
    } else {
        // USB mice report screen-style Y (positive is down); pass the delta
        // through unchanged.
        if x != 0 || y != 0 {
            crate::input::move_mouse(x, y);
        }
    }

    crate::input::set_mouse_buttons(buttons);

    if let Some(wheel) = mouse.wheel {
        let delta = axis(report, &wheel) as i16;

        if delta != 0 {
            crate::input::mouse_wheel(delta);
        }
    }
}

fn axis(report: &[u8], field: &Field) -> i32 {
    let raw = extract(report, field.offset, field.size);

    if field.logical_min < 0 {
        sign_extend_bits(raw, field.size)
    } else {
        raw as i32
    }
}

/// Scale an absolute axis value into framebuffer pixels.
fn scale_axis(value: i32, logical_max: i32, extent: i32) -> i32 {
    if logical_max <= 0 || extent <= 0 {
        return value;
    }

    let value = value.clamp(0, logical_max) as i64;
    let extent = extent as i64;

    ((value * extent) / logical_max as i64) as i32
}

/// Extract `size` bits (up to 32) starting at `offset`, LSB first.
fn extract(report: &[u8], offset: usize, size: usize) -> u32 {
    let mut value = 0u32;

    for bit in 0..size.min(32) {
        let position = offset + bit;
        let byte = position / 8;

        if byte >= report.len() {
            break;
        }

        let set = (report[byte] >> (position % 8)) & 1;

        value |= (set as u32) << bit;
    }

    value
}

fn sign_extend(value: u32, size: usize) -> i32 {
    if size == 0 {
        return 0;
    }

    if size >= 4 {
        return value as i32;
    }

    sign_extend_bits(value, size * 8)
}

fn sign_extend_bits(value: u32, bits: usize) -> i32 {
    if bits == 0 {
        return 0;
    }

    if bits >= 32 {
        return value as i32;
    }

    let sign = 1u32 << (bits - 1);

    ((value ^ sign).wrapping_sub(sign)) as i32
}
