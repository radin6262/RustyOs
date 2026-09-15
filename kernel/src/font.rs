use alloc::{vec, vec::Vec};
use core::cell::UnsafeCell;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

// ============================================================
// Embedded font
// ============================================================

static FONT_DATA: &[u8] =
    include_bytes!("../assets/CascadiaMono.ttf");

// ============================================================
// Public glyph metrics
// ============================================================

pub struct GlyphMetrics {
    pub width: usize,
    pub height: usize,

    pub offset_x: i32,
    pub offset_y: i32,

    pub advance_width: f32,
}

// ============================================================
// Cached glyph
// ============================================================

struct CachedGlyph {
    pixel_size: f32,
    metrics: GlyphMetrics,
    bitmap: Vec<u8>,
}

// ============================================================
// Font state
// ============================================================

struct FontState {
    face: Face<'static>,

    // Heap allocated instead of large fixed arrays on the
    // kernel stack.
    small: Vec<Option<CachedGlyph>>,
    large: Vec<Option<CachedGlyph>>,
}

// ============================================================
// Global font storage
// ============================================================

struct FontStorage {
    state: UnsafeCell<Option<FontState>>,
}

unsafe impl Sync for FontStorage {}

impl FontStorage {
    const fn new() -> Self {
        Self {
            state: UnsafeCell::new(None),
        }
    }
}

static FONT: FontStorage = FontStorage::new();

// ============================================================
// Outline segment
// ============================================================

#[derive(Clone, Copy)]
struct Segment {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

// ============================================================
// Outline collector
// ============================================================

struct Outline {
    segments: Vec<Segment>,
    current_x: f32,
    current_y: f32,
    start_x: f32,
    start_y: f32,
}

impl Outline {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            current_x: 0.0,
            current_y: 0.0,
            start_x: 0.0,
            start_y: 0.0,
        }
    }

    fn push_line(&mut self, x: f32, y: f32) {
        self.segments.push(Segment {
            x0: self.current_x,
            y0: self.current_y,
            x1: x,
            y1: y,
        });

        self.current_x = x;
        self.current_y = y;
    }

    fn flatten_quad(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) {
        const STEPS: usize = 8;

        let mut previous_x = x0;
        let mut previous_y = y0;

        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let inv = 1.0 - t;

            let x =
                inv * inv * x0
                    + 2.0 * inv * t * x1
                    + t * t * x2;

            let y =
                inv * inv * y0
                    + 2.0 * inv * t * y1
                    + t * t * y2;

            self.segments.push(Segment {
                x0: previous_x,
                y0: previous_y,
                x1: x,
                y1: y,
            });

            previous_x = x;
            previous_y = y;
        }

        self.current_x = x2;
        self.current_y = y2;
    }

    fn flatten_cubic(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) {
        const STEPS: usize = 12;

        let mut previous_x = x0;
        let mut previous_y = y0;

        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let inv = 1.0 - t;

            let x =
                inv * inv * inv * x0
                    + 3.0 * inv * inv * t * x1
                    + 3.0 * inv * t * t * x2
                    + t * t * t * x3;

            let y =
                inv * inv * inv * y0
                    + 3.0 * inv * inv * t * y1
                    + 3.0 * inv * t * t * y2
                    + t * t * t * y3;

            self.segments.push(Segment {
                x0: previous_x,
                y0: previous_y,
                x1: x,
                y1: y,
            });

            previous_x = x;
            previous_y = y;
        }

        self.current_x = x3;
        self.current_y = y3;
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.current_x != self.start_x
            || self.current_y != self.start_y
        {
            self.segments.push(Segment {
                x0: self.current_x,
                y0: self.current_y,
                x1: self.start_x,
                y1: self.start_y,
            });
        }

        self.current_x = x;
        self.current_y = y;

        self.start_x = x;
        self.start_y = y;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_line(x, y);
    }

    fn quad_to(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) {
        self.flatten_quad(
            self.current_x,
            self.current_y,
            x1,
            y1,
            x2,
            y2,
        );
    }

    fn curve_to(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) {
        self.flatten_cubic(
            self.current_x,
            self.current_y,
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        );
    }

    fn close(&mut self) {
        if self.current_x != self.start_x
            || self.current_y != self.start_y
        {
            self.segments.push(Segment {
                x0: self.current_x,
                y0: self.current_y,
                x1: self.start_x,
                y1: self.start_y,
            });
        }

        self.current_x = self.start_x;
        self.current_y = self.start_y;
    }
}

// ============================================================
// Float helpers
// ============================================================

fn floor_f32(value: f32) -> i32 {
    let integer = value as i32;

    if value < integer as f32 {
        integer - 1
    } else {
        integer
    }
}

fn ceil_f32(value: f32) -> i32 {
    let integer = value as i32;

    if value > integer as f32 {
        integer + 1
    } else {
        integer
    }
}

// ============================================================
// Initialization
// ============================================================

pub fn init() {
    unsafe {
        if (*FONT.state.get()).is_some() {
            return;
        }
    }

    crate::serial::write_str(
        "font: parsing CascadiaMono.ttf\n",
    );

    let face =
        Face::parse(FONT_DATA, 0)
            .expect("Rusty: invalid TTF font");

    crate::serial::write_str(
        "font: TTF parsed\n",
    );

    // Allocate the cache containers on the heap.
    //
    // resize_with() is used instead of vec![None; 128]
    // because CachedGlyph itself is not Clone.
    let mut small =
        Vec::<Option<CachedGlyph>>::with_capacity(128);

    small.resize_with(128, || None);

    crate::serial::write_str(
        "font: small cache allocated\n",
    );

    let mut large =
        Vec::<Option<CachedGlyph>>::with_capacity(128);

    large.resize_with(128, || None);

    crate::serial::write_str(
        "font: large cache allocated\n",
    );

    let state =
        FontState {
            face,
            small,
            large,
        };

    crate::serial::write_str(
        "font: state constructed\n",
    );

    unsafe {
        *FONT.state.get() = Some(state);
    }

    crate::serial::write_str(
        "font: initialization complete\n",
    );
}

// ============================================================
// State
// ============================================================

unsafe fn state_mut() -> &'static mut FontState {
    unsafe {
        (*FONT.state.get())
            .as_mut()
            .expect(
                "Rusty: font not initialized",
            )
    }
}

// ============================================================
// Rasterization
// ============================================================

fn build_glyph(
    face: &Face<'static>,
    character: char,
    pixel_size: f32,
) -> Option<CachedGlyph> {
    let glyph_id: GlyphId =
        face.glyph_index(character)?;

    let advance_units =
        face.glyph_hor_advance(glyph_id)
            .unwrap_or(0);

    let units_per_em =
        face.units_per_em() as f32;

    let scale =
        pixel_size / units_per_em;

    let bbox =
        face.glyph_bounding_box(glyph_id)?;

    let width_f =
        (bbox.x_max - bbox.x_min) as f32
            * scale;

    let height_f =
        (bbox.y_max - bbox.y_min) as f32
            * scale;

    let width =
        ceil_f32(width_f)
            .max(0) as usize;

    let height =
        ceil_f32(height_f)
            .max(0) as usize;

    let offset_x =
        floor_f32(
            bbox.x_min as f32 * scale,
        );

    let offset_y =
        -ceil_f32(
            bbox.y_max as f32 * scale,
        );

    let advance_width =
        advance_units as f32 * scale;

    if width == 0 || height == 0 {
        return Some(CachedGlyph {
            pixel_size,

            metrics: GlyphMetrics {
                width,
                height,
                offset_x,
                offset_y,
                advance_width,
            },

            bitmap: Vec::new(),
        });
    }

    let mut outline =
        Outline::new();

    face.outline_glyph(
        glyph_id,
        &mut outline,
    )?;

    let pixel_count =
        width.checked_mul(height)?;

    let mut bitmap =
        vec![0u8; pixel_count];

    const SAMPLES: usize = 4;

    let sample_count =
        (SAMPLES * SAMPLES) as u16;

    for py in 0..height {
        for px in 0..width {
            let mut inside = 0u16;

            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let local_x =
                        px as f32
                            + (sx as f32 + 0.5)
                            / SAMPLES as f32;

                    let local_y =
                        py as f32
                            + (sy as f32 + 0.5)
                            / SAMPLES as f32;

                    let font_x =
                        bbox.x_min as f32
                            + local_x / scale;

                    let font_y =
                        bbox.y_max as f32
                            - local_y / scale;

                    if point_inside(
                        &outline.segments,
                        font_x,
                        font_y,
                    ) {
                        inside += 1;
                    }
                }
            }

            if inside != 0 {
                bitmap[
                    py * width + px
                    ] =
                    (inside * 255
                        / sample_count)
                        as u8;
            }
        }
    }

    Some(CachedGlyph {
        pixel_size,

        metrics: GlyphMetrics {
            width,
            height,
            offset_x,
            offset_y,
            advance_width,
        },

        bitmap,
    })
}

// ============================================================
// Point in glyph
// ============================================================

fn point_inside(
    segments: &[Segment],
    x: f32,
    y: f32,
) -> bool {
    let mut winding = 0i32;

    for segment in segments {
        let x0 = segment.x0;
        let y0 = segment.y0;

        let x1 = segment.x1;
        let y1 = segment.y1;

        if y0 == y1 {
            continue;
        }

        let crosses =
            (y0 <= y && y1 > y)
                || (y0 > y && y1 <= y);

        if !crosses {
            continue;
        }

        let intersect_x =
            x0
                + (y - y0)
                * (x1 - x0)
                / (y1 - y0);

        if intersect_x > x {
            if y1 > y0 {
                winding += 1;
            } else {
                winding -= 1;
            }
        }
    }

    winding != 0
}

// ============================================================
// Get glyph
// ============================================================

pub fn rasterize(
    character: char,
    pixel_size: f32,
) -> Option<(GlyphMetrics, &'static [u8])> {
    init();

    let code =
        character as u32;

    if code >= 128 {
        return None;
    }

    let index =
        code as usize;

    unsafe {
        let state =
            state_mut();

        let face_ptr:
            *const Face<'static> =
            &state.face;

        let cache =
            if pixel_size <= 16.0 {
                &mut state.small
            } else {
                &mut state.large
            };

        let needs_build =
            match &cache[index] {
                Some(glyph) => {
                    (glyph.pixel_size
                        - pixel_size)
                        .abs()
                        > 0.01
                }

                None => true,
            };

        if needs_build {
            let glyph =
                build_glyph(
                    &*face_ptr,
                    character,
                    pixel_size,
                )?;

            cache[index] =
                Some(glyph);
        }

        let glyph =
            cache[index]
                .as_ref()
                .unwrap();

        let metrics =
            GlyphMetrics {
                width:
                glyph.metrics.width,

                height:
                glyph.metrics.height,

                offset_x:
                glyph.metrics.offset_x,

                offset_y:
                glyph.metrics.offset_y,

                advance_width:
                glyph.metrics.advance_width,
            };

        let ptr =
            glyph.bitmap.as_ptr();

        let len =
            glyph.bitmap.len();

        Some((
            metrics,
            core::slice::from_raw_parts(
                ptr,
                len,
            ),
        ))
    }
}

// ============================================================
// Advance
// ============================================================

pub fn advance(
    character: char,
    pixel_size: f32,
) -> f32 {
    init();

    unsafe {
        let state =
            state_mut();

        let Some(glyph_id) =
            state.face.glyph_index(
                character,
            )
        else {
            return 0.0;
        };

        let advance =
            state.face
                .glyph_hor_advance(
                    glyph_id,
                )
                .unwrap_or(0);

        advance as f32
            * pixel_size
            / state.face.units_per_em()
            as f32
    }
}

// ============================================================
// Line metrics
// ============================================================

pub fn ascent(
    pixel_size: f32,
) -> f32 {
    init();

    unsafe {
        let state =
            state_mut();

        state.face.ascender()
            as f32
            * pixel_size
            / state.face.units_per_em()
            as f32
    }
}

pub fn line_height(
    pixel_size: f32,
) -> usize {
    init();

    unsafe {
        let state =
            state_mut();

        let height =
            state.face.height()
                as f32
                * pixel_size
                / state.face.units_per_em()
                as f32;

        ceil_f32(height)
            .max(1) as usize
    }
}