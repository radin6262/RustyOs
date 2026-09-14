use alloc::vec::Vec;
use core::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicU8,
        Ordering,
    },
};

use bootloader_api::info::{
    FrameBuffer,
    PixelFormat,
};

struct GraphicsBuffers {
    frame: Vec<u8>,
    base: Vec<u8>,

    width: usize,
    height: usize,
    stride: usize,
    bytes_per_pixel: usize,
    pixel_format: PixelFormat,
}

struct GraphicsStorage {
    state: UnsafeCell<Option<GraphicsBuffers>>,
}

unsafe impl Sync for GraphicsStorage {}

impl GraphicsStorage {
    const fn new() -> Self {
        Self {
            state: UnsafeCell::new(None),
        }
    }
}

static STORAGE: GraphicsStorage =
    GraphicsStorage::new();

static INITIALIZED: AtomicU8 =
    AtomicU8::new(0);

pub fn init(
    framebuffer: &mut FrameBuffer,
) {
    let info =
        framebuffer.info();

    let size =
        info.stride
            .checked_mul(info.height)
            .and_then(|pixels|
                pixels.checked_mul(
                    info.bytes_per_pixel,
                ))
            .expect(
                "Rusty: framebuffer size overflow",
            );

    let frame =
        alloc::vec![0u8; size];

    let base =
        alloc::vec![0u8; size];

    unsafe {
        *STORAGE.state.get() =
            Some(GraphicsBuffers {
                frame,
                base,

                width: info.width,
                height: info.height,
                stride: info.stride,
                bytes_per_pixel:
                info.bytes_per_pixel,
                pixel_format:
                info.pixel_format,
            });
    }

    INITIALIZED.store(
        1,
        Ordering::Release,
    );
}

#[inline(always)]
pub fn is_initialized() -> bool {
    INITIALIZED.load(
        Ordering::Acquire,
    ) != 0
}

#[inline(always)]
pub fn width() -> usize {
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .map(|state| state.width)
            .unwrap_or(0)
    }
}

#[inline(always)]
pub fn height() -> usize {
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .map(|state| state.height)
            .unwrap_or(0)
    }
}

#[inline(always)]
pub fn stride() -> usize {
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .map(|state| state.stride)
            .unwrap_or(0)
    }
}

#[inline(always)]
pub fn bytes_per_pixel() -> usize {
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .map(|state|
                state.bytes_per_pixel
            )
            .unwrap_or(0)
    }
}

#[inline(always)]
pub fn pixel_format() -> PixelFormat {
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .map(|state|
                state.pixel_format
            )
            .unwrap_or(PixelFormat::Rgb)
    }
}

// ============================================================
// Frame buffer access
// ============================================================

pub(crate) unsafe fn frame_ptr()
    -> *mut u8
{
    unsafe {
        (*STORAGE.state.get())
            .as_mut()
            .expect(
                "Rusty: graphics not initialized",
            )
            .frame
            .as_mut_ptr()
    }
}

pub(crate) unsafe fn frame_len()
    -> usize
{
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .expect(
                "Rusty: graphics not initialized",
            )
            .frame
            .len()
    }
}

pub(crate) unsafe fn base_ptr()
    -> *mut u8
{
    unsafe {
        (*STORAGE.state.get())
            .as_mut()
            .expect(
                "Rusty: graphics not initialized",
            )
            .base
            .as_mut_ptr()
    }
}

pub(crate) unsafe fn base_len()
    -> usize
{
    unsafe {
        (*STORAGE.state.get())
            .as_ref()
            .expect(
                "Rusty: graphics not initialized",
            )
            .base
            .len()
    }
}

// ============================================================
// Base frame
// ============================================================

pub unsafe fn base_mut()
    -> &'static mut [u8]
{
    unsafe {
        let state =
            (*STORAGE.state.get())
                .as_mut()
                .expect(
                    "Rusty: graphics not initialized",
                );

        core::slice::from_raw_parts_mut(
            state.base.as_mut_ptr(),
            state.base.len(),
        )
    }
}

// ============================================================
// Frame operations
// ============================================================

pub fn begin_frame() {
    if !is_initialized() {
        return;
    }

    unsafe {
        let destination =
            core::slice::from_raw_parts_mut(
                frame_ptr(),
                frame_len(),
            );

        let source =
            core::slice::from_raw_parts(
                base_ptr(),
                base_len(),
            );

        destination.copy_from_slice(
            source,
        );
    }
}

pub fn present(
    framebuffer: &mut FrameBuffer,
) {
    if !is_initialized() {
        return;
    }

    let destination =
        framebuffer.buffer_mut();

    unsafe {
        let source =
            core::slice::from_raw_parts(
                frame_ptr(),
                frame_len(),
            );

        let size =
            destination
                .len()
                .min(source.len());

        destination[..size]
            .copy_from_slice(
                &source[..size],
            );
    }
}