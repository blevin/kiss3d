// This whole file is strongly inspired by: https://github.com/jeaye/q3/blob/master/src/client/ui/ttf/font.rs
// available under the BSD-3 licence.
// It has been modified to work with gl-rs, nalgebra, and rust-freetype

use std::rc::Rc;
use std::num::UnsignedInt;
use std::cmp;
use std::ptr;
use std::kinds::marker::NoCopy;
use libc::{c_uint, c_void};
use gl;
use gl::types::*;
use freetype::ffi;
use na::Vec2;
use na;
use text::Glyph;

#[path = "../error.rs"]
mod error;

/// A ttf font.
pub struct Font {
    library:          ffi::FT_Library,
    face:             ffi::FT_Face,
    texture_atlas:    GLuint,
    atlas_dimensions: Vec2<uint>,
    glyphs:           Vec<Option<Glyph>>,
    height:           i32,
    nocpy:            NoCopy
}

impl Font {
    /// Loads a new ttf font from the memory.
    pub fn from_memory(font: &[u8], size: i32) -> Rc<Font> {
        Font::do_new(None, font, size)
    }

    /// Loads a new ttf font from a file.
    pub fn new(path: &Path, size: i32) -> Rc<Font> {
        Font::do_new(Some(path), &[], size)
    }

    /// Loads a new ttf font from a file.
    pub fn do_new(path: Option<&Path>, memory: &[u8], size: i32) -> Rc<Font> {
        let mut font = Font {
            library:          ptr::null_mut(),
            face:             ptr::null_mut(),
            texture_atlas:    0,
            atlas_dimensions: na::zero(),
            glyphs:           range(0, 128).map(|_:int| None).collect(),
            height:           0,
            nocpy:            NoCopy
        };

        unsafe {
            let _ = ffi::FT_Init_FreeType(&mut font.library);

            match path {
                Some(path) => {
                    let c_str = path.as_str().expect("Invalid path.").to_c_str();
                    if ffi::FT_New_Face(font.library, c_str.as_ptr(), 0, &mut font.face) != 0 {
                        panic!("Failed to create TTF face.");
                    }
                },
                None => {
                    if ffi::FT_New_Memory_Face(font.library, &memory[0], memory.len() as i64, 0, &mut font.face) != 0 {
                        panic!("Failed to create TTF face.");
                    }
                }
            }

            let _ = ffi::FT_Set_Pixel_Sizes(font.face, 0, size as c_uint);
            verify!(gl::ActiveTexture(gl::TEXTURE0));

            let     ft_glyph   = (*font.face).glyph;
            let     max_width  = 1024;
            let mut row_width  = 0;
            let mut row_height = 0;

            for curr in range(0u, 128) {
                if ffi::FT_Load_Char(font.face, curr as u64, ffi::FT_LOAD_RENDER) != 0 {
                    continue;
                }

                /* If we've exhausted the width for this row, add another. */
                if row_width + (*ft_glyph).bitmap.width + 1 >= max_width {
                    font.atlas_dimensions.x = cmp::max(font.atlas_dimensions.x, row_width as uint);
                    font.atlas_dimensions.y = font.atlas_dimensions.y + row_height;
                    row_width = 0; row_height = 0;
                }

                let advance    = Vec2::new(((*ft_glyph).advance.x >> 6) as f32, ((*ft_glyph).advance.y >> 6) as f32);
                let dimensions = Vec2::new((*ft_glyph).bitmap.width as f32, (*ft_glyph).bitmap.rows as f32);
                let offset     = Vec2::new((*ft_glyph).bitmap_left as f32, (*ft_glyph).bitmap_top as f32);
                let buffer     = Vec::from_raw_buf(&*(*ft_glyph).bitmap.buffer, (dimensions.x * dimensions.y) as uint);
                let glyph      = Glyph::new(na::zero(), advance, dimensions, offset, buffer);
                    

                row_width   = row_width + (dimensions.x + 1.0) as i32;
                row_height  = cmp::max(row_height, (*ft_glyph).bitmap.rows as uint);
                font.height = cmp::max(font.height, row_height as i32);

                font.glyphs[curr] = Some(glyph);
            }

            font.atlas_dimensions.x = UnsignedInt::next_power_of_two(cmp::max(font.atlas_dimensions.x, row_width as uint));
            font.atlas_dimensions.y = UnsignedInt::next_power_of_two(font.atlas_dimensions.y + row_height);

            /* We're using 1 byte alignment buffering. */
            verify!(gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1));

            verify!(gl::GenTextures(1, &mut font.texture_atlas));
            verify!(gl::BindTexture(gl::TEXTURE_2D, font.texture_atlas));
            verify!(gl::TexImage2D(gl::TEXTURE_2D, 0, gl::RGB as GLint,
                                   font.atlas_dimensions.x as i32, font.atlas_dimensions.y as i32,
                                   0, gl::RED, gl::UNSIGNED_BYTE, ptr::null()));

            /* Clamp to the edge to avoid artifacts when scaling. */
            verify!(gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32));
            verify!(gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32));

            /* Linear filtering usually looks best for text. */
            verify!(gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32));
            verify!(gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32));

            /* Copy all glyphs into the texture atlas. */
            let mut offset: Vec2<i32> = na::zero();
            row_height = 0;
            for curr in range(0u, 128) {
                let glyph = match *&mut font.glyphs[curr] {
                    Some(ref mut g) => g,
                    None            => continue
                };

                if offset.x + (glyph.dimensions.x as i32) + 1 >= max_width {
                    offset.y   = offset.y + row_height as i32;
                    row_height = 0;
                    offset.x   = 0;
                }

                if !glyph.buffer.is_empty() {
                    verify!(gl::TexSubImage2D(
                                gl::TEXTURE_2D, 0, offset.x, offset.y,
                                glyph.dimensions.x as i32, glyph.dimensions.y as i32,
                                gl::RED, gl::UNSIGNED_BYTE, &glyph.buffer[0] as *const u8 as *const c_void));
                }

                /* Calculate the position in the texture. */
                glyph.tex.x = offset.x as f32 / (font.atlas_dimensions.x as f32);
                glyph.tex.y = offset.y as f32 / (font.atlas_dimensions.y as f32);

                offset.x   = offset.x + glyph.dimensions.x as i32;
                row_height = cmp::max(row_height, glyph.dimensions.y as uint);
            }
        }

        /* Reset the state. */
        verify!(gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4));

        assert!(font.height > 0);

        Rc::new(font)
    }

    /// The opengl id to the texture atlas of this font.
    #[inline]
    pub fn texture_atlas(&self) -> GLuint {
        self.texture_atlas
    }

    /// The dimensions of the texture atlas of this font.
    #[inline]
    pub fn atlas_dimensions(&self) -> Vec2<uint> {
        self.atlas_dimensions
    }

    /// The glyphs of the this font.
    #[inline]
    pub fn glyphs<'a>(&'a self) -> &'a [Option<Glyph>] {
        self.glyphs.as_slice()
    }

    /// The height of this font.
    #[inline]
    pub fn height(&self) -> i32 {
        self.height
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = ffi::FT_Done_FreeType(self.library);
            verify!(gl::DeleteTextures(1, &self.texture_atlas));
        }
    }
}
