use fontdue::{Font, FontSettings};
use tiny_skia::{Pixmap, ColorU8};

// Embed a compact monospaced font at compile time.
// Using JetBrains Mono — replace with any OFL/MIT-licensed TTF.
static FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Lazy-initialised font handle.
pub struct BitmapFont {
    font: Font,
}

impl BitmapFont {
    pub fn new() -> Self {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("Failed to load embedded font");
        Self { font }
    }

    /// Rasterise `text` at `px` size and paint it onto `pixmap` at (x, y).
    /// `color` is [R, G, B, A] — 0-255 each.
    pub fn draw(
        &self,
        pixmap: &mut Pixmap,
        text:   &str,
        mut x:  i32,
        y:      i32,
        px:     f32,
        color:  [u8; 4],
    ) {
        let pw = pixmap.width()  as i32;
        let ph = pixmap.height() as i32;

        for ch in text.chars() {
            let (metrics, bitmap) = self.font.rasterize(ch, px);

            let x_off = x + metrics.xmin;
            let y_off = y - metrics.height as i32 - metrics.ymin;

            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let alpha = bitmap[row * metrics.width + col];
                    if alpha == 0 { continue; }

                    let px_x = x_off + col as i32;
                    let px_y = y_off + row as i32;

                    if px_x < 0 || px_y < 0 || px_x >= pw || px_y >= ph {
                        continue;
                    }

                    // Alpha-blend onto pixmap
                    let idx = (px_y as u32 * pixmap.width() + px_x as u32) as usize;
                    let pixels = pixmap.pixels_mut();

                    let src_a  = alpha as u32 * color[3] as u32 / 255;
                    let dst_a  = pixels[idx].alpha() as u32;
                    let out_a  = src_a + dst_a * (255 - src_a) / 255;

                    let blend = |src: u8, dst: u8| -> u8 {
                        if out_a == 0 { return 0; }
                        ((src as u32 * src_a + dst as u32 * dst_a * (255 - src_a) / 255) / out_a) as u8
                    };

                    pixels[idx] = ColorU8::from_rgba(
                        blend(color[0], pixels[idx].red()),
                        blend(color[1], pixels[idx].green()),
                        blend(color[2], pixels[idx].blue()),
                        out_a as u8,
                    ).premultiply();
                }
            }

            // Advance cursor by glyph advance width
            x += metrics.advance_width as i32;
        }
    }

    /// Measure the pixel width of a string at the given size.
    #[allow(dead_code)]
    pub fn measure_width(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|ch| self.font.rasterize(ch, px).0.advance_width)
            .sum()
    }
}
