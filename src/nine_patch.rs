use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Patch {
    pub size: [u32; 2],
    /// Left, top, right, bottom in source pixels.
    pub insets: [u32; 4],
    pub png_base64: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Result,
        native::{self, Draw, Scene, rgb},
        project::encode_base64,
    };

    #[test]
    fn resized_patch_preserves_four_source_corners() -> Result<()> {
        let mut source = Scene::new(20, 20);
        for (rect, color) in [
            ([0.0, 0.0, 20.0, 20.0], 0x333333),
            ([0.0, 0.0, 4.0, 4.0], 0xff8844),
            ([16.0, 0.0, 4.0, 4.0], 0x66cc44),
            ([0.0, 16.0, 4.0, 4.0], 0x5588ff),
            ([16.0, 16.0, 4.0, 4.0], 0xffcc66),
        ] {
            source.rect(rect, 0.0, rgb(color));
        }
        let patch = Patch {
            size: [20, 20],
            insets: [4; 4],
            png_base64: encode_base64(&native::png(&source)?),
        };
        let raster = |width, height| -> Result<Vec<u8>> {
            let mut scene = Scene::new(width, height);
            scene.commands.push(Draw::NinePatch {
                rect: [0.0, 0.0, width as f32, height as f32],
                edge_scale: 1.0,
                patch: patch.clone(),
            });
            let data = native::png(&scene)?;
            let mut reader = png::Decoder::new(data.as_slice()).read_info()?;
            let mut pixels = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut pixels)?;
            Ok(pixels)
        };
        let small = raster(60, 30)?;
        let large = raster(120, 75)?;
        for (small_x, large_x) in [(0, 0), (56, 116)] {
            for (small_y, large_y) in [(0, 0), (26, 71)] {
                for row in 0..4 {
                    let first = ((small_y + row) * 60 + small_x) * 4;
                    let second = ((large_y + row) * 120 + large_x) * 4;
                    assert_eq!(&small[first..first + 16], &large[second..second + 16]);
                }
            }
        }
        Ok(())
    }
}
