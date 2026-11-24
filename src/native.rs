use crate::{
    Result,
    nine_patch::Patch,
    project::{Appearance, NinePatch, WorkspaceFont, sprite_size},
};
use ab_glyph::{Font, FontArc, FontVec, PxScale, ScaleFont, VariableFont, point};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

pub type Color = [u8; 4];
pub type Point = [f32; 2];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Color,
    Grayscale,
}

impl RenderMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "color" => Ok(Self::Color),
            "grayscale" => Ok(Self::Grayscale),
            _ => Err("Render mode must be color or grayscale".into()),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Grayscale => "grayscale",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_preserves_alpha_and_weights_colors() -> Result<()> {
        let mut scene = Scene::new(2, 1);
        scene.rect([0.0, 0.0, 1.0, 1.0], 0.0, [255, 0, 0, 128]);
        scene.rect([1.0, 0.0, 1.0, 1.0], 0.0, [0, 255, 0, 255]);
        let data = png_mode(&scene, RenderMode::Grayscale)?;
        let mut reader = png::Decoder::new(data.as_slice()).read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut pixels)?;

        assert_eq!(pixels, [54, 54, 54, 128, 182, 182, 182, 255]);
        Ok(())
    }
}

pub const fn rgb(value: u32) -> Color {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}

#[derive(Serialize)]
pub struct Scene {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub commands: Vec<Draw>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fonts: BTreeMap<String, WorkspaceFont>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Draw {
    Polygon {
        points: Vec<Point>,
        color: Color,
    },
    Ellipse {
        center: Point,
        radii: Point,
        color: Color,
    },
    RoundedRect {
        rect: [f32; 4],
        radius: f32,
        color: Color,
    },
    Text {
        at: Point,
        size: f32,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        font: Option<String>,
        color: Color,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        centered: bool,
    },
    Sprite {
        rect: [f32; 4],
        png_base64: String,
    },
    NinePatch {
        rect: [f32; 4],
        edge_scale: f32,
        patch: Patch,
    },
}

impl Scene {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            version: 1,
            width,
            height,
            commands: Vec::new(),
            fonts: BTreeMap::new(),
        }
    }

    pub fn limit_resolution(&mut self, longest_side: u32) {
        let largest = self.width.max(self.height);
        if largest <= longest_side || longest_side == 0 {
            return;
        }

        let scale = longest_side as f32 / largest as f32;
        self.width = (self.width as f32 * scale).round().max(1.0) as u32;
        self.height = (self.height as f32 * scale).round().max(1.0) as u32;
        transform_draws(&mut self.commands, [0.0, 0.0], scale);
    }

    pub fn polygon(&mut self, points: &[Point], color: Color) {
        self.commands.push(Draw::Polygon {
            points: points.to_vec(),
            color,
        });
    }

    pub fn ellipse(&mut self, center: Point, radii: Point, color: Color) {
        self.commands.push(Draw::Ellipse {
            center,
            radii,
            color,
        });
    }

    pub fn rect(&mut self, rect: [f32; 4], radius: f32, color: Color) {
        self.commands.push(Draw::RoundedRect {
            rect,
            radius,
            color,
        });
    }
}

pub(crate) fn transform_draws(draws: &mut [Draw], [left, top]: Point, scale: f32) {
    let point = |at: &mut Point| {
        at[0] = left + at[0] * scale;
        at[1] = top + at[1] * scale;
    };
    let rectangle = |rect: &mut [f32; 4]| {
        *rect = [
            left + rect[0] * scale,
            top + rect[1] * scale,
            rect[2] * scale,
            rect[3] * scale,
        ];
    };
    for draw in draws {
        match draw {
            Draw::Polygon { points, .. } => points.iter_mut().for_each(point),
            Draw::Ellipse { center, radii, .. } => {
                point(center);
                radii[0] *= scale;
                radii[1] *= scale;
            }
            Draw::RoundedRect { rect, radius, .. } => {
                rectangle(rect);
                *radius *= scale;
            }
            Draw::Text { at, size, .. } => {
                point(at);
                *size *= scale;
            }
            Draw::NinePatch {
                rect, edge_scale, ..
            } => {
                rectangle(rect);
                *edge_scale *= scale;
            }
            Draw::Sprite { rect, .. } => rectangle(rect),
        }
    }
}

pub fn scene(snapshot: &Value) -> Result<Scene> {
    let screen = &snapshot["screen"];
    let width = screen["width"]
        .as_f64()
        .ok_or("Select a screen to render")?
        .ceil() as u32;
    let height = screen["height"]
        .as_f64()
        .ok_or("Select a screen to render")?
        .ceil() as u32;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width as u64 * height as u64 > 16_777_216
    {
        return Err("Native canvas is limited to 8192 pixels per side and 16 megapixels".into());
    }

    let mut scene = Scene::new(width, height);
    if let Some(fonts) = snapshot.get("fonts") {
        scene.fonts = serde_json::from_value(fonts.clone())?;
    }
    let canvas = [0.0, 0.0, width as f32, height as f32];
    scene.rect(canvas, 0.0, parse_color(&screen["background"], 0xffffff));
    for element in screen["elements"]
        .as_array()
        .ok_or("Screen requires elements")?
    {
        let rect = ["x", "y", "width", "height"]
            .map(|field| element[field].as_f64().unwrap_or_default() as f32);
        let kind = element["type"].as_str().unwrap_or_default();
        if kind == "hotspot" {
            continue;
        }
        let appearance = element
            .get("appearance")
            .map(|value| serde_json::from_value::<Appearance>(value.clone()))
            .transpose()?
            .unwrap_or_default();
        if ![
            "panel", "text", "button", "sprite", "input", "checkbox", "select",
        ]
        .contains(&kind)
        {
            return Err(format!("Native rendering does not support {kind} elements").into());
        }

        let [x, y, width, height] = rect;
        let control = ["button", "input", "checkbox", "select"].contains(&kind);
        let radius = appearance
            .radius
            .map_or(if control { 6.0 } else { 0.0 }, f32::from);
        if let Some(patch_id) = appearance.patch.as_deref() {
            let definition: NinePatch =
                serde_json::from_value(snapshot["ninePatches"][patch_id].clone())?;
            let sprite = sprite_for(snapshot, &definition.sprite)?;
            scene.commands.push(Draw::NinePatch {
                rect,
                edge_scale: definition.edge_scale,
                patch: Patch {
                    size: sprite_size(sprite)?,
                    insets: definition.insets,
                    png_base64: sprite["data"]
                        .as_str()
                        .ok_or("Missing patch data")?
                        .to_owned(),
                },
            });
        } else if control || appearance.fill.is_some() {
            let fill = appearance.fill.unwrap_or(rgb(0xe9e5dd));
            let cut = f32::from(appearance.cut.unwrap_or_default())
                .min(width / 2.0)
                .min(height / 2.0);
            if cut > 0.0 {
                scene.polygon(
                    &[
                        [x + cut, y],
                        [x + width, y],
                        [x + width, y + height - cut],
                        [x + width - cut, y + height],
                        [x, y + height],
                        [x, y + cut],
                    ],
                    fill,
                );
            } else {
                scene.rect(rect, radius, fill);
            }
        }
        if let Some(sprite_id) = element["sprite"].as_str() {
            let asset = sprite_for(snapshot, sprite_id)?;
            if asset["type"] != "png" {
                return Err("Native sprite drawing requires PNG assets".into());
            }
            scene.commands.push(Draw::Sprite {
                rect,
                png_base64: asset["data"]
                    .as_str()
                    .ok_or("Missing sprite data")?
                    .to_owned(),
            });
        }

        let text = match kind {
            "input" | "select" => element["value"].as_str().or(element["text"].as_str()),
            "checkbox" => element["value"]
                .as_bool()
                .filter(|checked| *checked)
                .map(|_| "✓"),
            "sprite" => None,
            _ => element["text"].as_str(),
        };
        if let Some(text) = text {
            let size = appearance.font_size.map_or(18.0, f32::from);
            let padding = appearance
                .padding
                .map_or(if control { 8.0 } else { 0.0 }, f32::from);
            let top = if control {
                (height - size).max(0.0) / 2.0
            } else {
                0.0
            };
            let font = appearance.font;
            let ink = appearance.ink.unwrap_or(rgb(0x302c26));
            for (index, line) in text.lines().enumerate() {
                let baseline = y + top + size + index as f32 * size * 1.24;
                let left = if appearance.centered {
                    x + width / 2.0
                } else {
                    x + padding
                };
                scene.commands.push(Draw::Text {
                    at: [left, baseline],
                    size,
                    text: line.to_owned(),
                    font: font.clone(),
                    color: ink,
                    centered: appearance.centered,
                });
            }
        }
    }
    Ok(scene)
}

fn sprite_for<'a>(snapshot: &'a Value, id: &str) -> Result<&'a Value> {
    snapshot["sprites"]
        .as_array()
        .and_then(|sprites| sprites.iter().find(|asset| asset["id"] == id))
        .ok_or_else(|| format!("Missing sprite {id}").into())
}

fn parse_color(value: &Value, default: u32) -> Color {
    let hex = value.as_str().and_then(|value| value.strip_prefix('#'));
    match hex {
        Some(value) if value.len() == 6 => u32::from_str_radix(value, 16).map_or(rgb(default), rgb),
        Some(value) if value.len() == 3 => {
            u32::from_str_radix(value, 16).map_or(rgb(default), |color| {
                let red = ((color >> 8) & 15) * 17;
                let green = ((color >> 4) & 15) * 17;
                let blue = (color & 15) * 17;
                rgb(red << 16 | green << 8 | blue)
            })
        }
        _ => rgb(default),
    }
}

pub fn png(scene: &Scene) -> Result<Vec<u8>> {
    png_mode(scene, RenderMode::Color)
}

pub fn png_mode(scene: &Scene, mode: RenderMode) -> Result<Vec<u8>> {
    Ok(profiled_png(scene, mode)?.0)
}

pub fn profiled_png(scene: &Scene, mode: RenderMode) -> Result<(Vec<u8>, Duration, Duration)> {
    let start = Instant::now();
    let mut raster = Raster {
        width: scene.width,
        height: scene.height,
        pixels: vec![0; scene.width as usize * scene.height as usize * 4],
    };
    let mut fonts = BTreeMap::new();
    for command in &scene.commands {
        match command {
            Draw::Polygon { points, color } => {
                if points.len() < 3 {
                    continue;
                }
                let bounds = [
                    points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                    points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                    points
                        .iter()
                        .map(|p| p[0])
                        .fold(f32::NEG_INFINITY, f32::max),
                    points
                        .iter()
                        .map(|p| p[1])
                        .fold(f32::NEG_INFINITY, f32::max),
                ];
                raster.shape(bounds, *color, |x, y| {
                    let mut inside = false;
                    let mut previous = points[points.len() - 1];
                    for &current in points {
                        if (current[1] > y) != (previous[1] > y)
                            && x < (previous[0] - current[0]) * (y - current[1])
                                / (previous[1] - current[1])
                                + current[0]
                        {
                            inside = !inside;
                        }
                        previous = current;
                    }
                    inside
                });
            }
            Draw::Ellipse {
                center: [x, y],
                radii: [rx, ry],
                color,
            } => {
                raster.shape([x - rx, y - ry, x + rx, y + ry], *color, |px, py| {
                    ((px - x) / rx).powi(2) + ((py - y) / ry).powi(2) <= 1.0
                });
            }
            Draw::RoundedRect {
                rect: [x, y, width, height],
                radius,
                color,
            } => {
                if *radius == 0.0
                    && color[3] == 255
                    && [*x, *y, *width, *height]
                        .into_iter()
                        .all(|value| value.fract() == 0.0)
                {
                    raster.fill_opaque([*x, *y, *width, *height], *color);
                    continue;
                }
                let radius = radius.max(0.0).min(width.min(*height) / 2.0);
                raster.shape([*x, *y, x + width, y + height], *color, |px, py| {
                    if px < *x || py < *y || px > x + width || py > y + height {
                        return false;
                    }
                    let dx = px - px.max(x + radius).min(x + width - radius);
                    let dy = py - py.max(y + radius).min(y + height - radius);
                    dx * dx + dy * dy <= radius * radius
                });
            }
            Draw::Text {
                at,
                size,
                text,
                font,
                color,
                centered,
            } => {
                let key = font.as_deref();
                let font = match fonts.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let source = key
                            .map(|id| {
                                scene
                                    .fonts
                                    .get(id)
                                    .ok_or_else(|| format!("Missing workspace font {id}"))
                            })
                            .transpose()?;
                        entry.insert(load_font(source)?)
                    }
                };
                raster.text(font, *at, *size, text, *color, *centered);
            }
            Draw::Sprite { rect, png_base64 } => raster.sprite(*rect, png_base64)?,
            Draw::NinePatch {
                rect,
                edge_scale,
                patch,
            } => raster.nine_patch(*rect, *edge_scale, patch)?,
        }
    }

    if mode == RenderMode::Grayscale {
        for pixel in raster.pixels.as_chunks_mut::<4>().0 {
            let value = (2126 * u32::from(pixel[0])
                + 7152 * u32::from(pixel[1])
                + 722 * u32::from(pixel[2])
                + 5000)
                / 10000;
            pixel[..3].fill(value as u8);
        }
    }

    let raster_time = start.elapsed();
    let encoding_start = Instant::now();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, scene.width, scene.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&raster.pixels)?;
    writer.finish()?;
    Ok((bytes, raster_time, encoding_start.elapsed()))
}

struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Raster {
    fn fill_opaque(&mut self, [x, y, width, height]: [f32; 4], color: Color) {
        let left = (x as u32).min(self.width) as usize;
        let right = ((x + width) as u32).min(self.width) as usize;
        if left >= right {
            return;
        }
        let stride = self.width as usize;
        for row in y as u32..((y + height) as u32).min(self.height) {
            let offset = row as usize * stride;
            for pixel in self.pixels[(offset + left) * 4..(offset + right) * 4]
                .as_chunks_mut::<4>()
                .0
            {
                *pixel = color;
            }
        }
    }

    fn shape(&mut self, bounds: [f32; 4], color: Color, inside: impl Fn(f32, f32) -> bool) {
        let [left, top, right, bottom] = bounds;
        for y in top.floor().max(0.0) as u32..(bottom.ceil() as u32).min(self.height) {
            for x in left.floor().max(0.0) as u32..(right.ceil() as u32).min(self.width) {
                let mut count = 0;
                for dy in [0.25, 0.75] {
                    for dx in [0.25, 0.75] {
                        if inside(x as f32 + dx, y as f32 + dy) {
                            count += 1;
                        }
                    }
                }
                if count != 0 {
                    self.blend(x as i32, y as i32, color, count as f32 / 4.0);
                }
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        if coverage >= 1.0 && color[3] == 255 {
            self.pixels[offset..offset + 4].copy_from_slice(&color);
            return;
        }
        let source = coverage.clamp(0.0, 1.0) * color[3] as f32 / 255.0;
        let destination = self.pixels[offset + 3] as f32 / 255.0 * (1.0 - source);
        let alpha = source + destination;
        if alpha == 0.0 {
            return;
        }
        for (channel, sample) in color.into_iter().take(3).enumerate() {
            let current = self.pixels[offset + channel] as f32;
            self.pixels[offset + channel] =
                ((sample as f32 * source + current * destination) / alpha).round() as u8;
        }
        self.pixels[offset + 3] = (alpha * 255.0).round() as u8;
    }

    fn text(
        &mut self,
        font: &FontArc,
        at: Point,
        size: f32,
        text: &str,
        color: Color,
        centered: bool,
    ) {
        let scaled = font.as_scaled(PxScale::from(size));
        let mut x = at[0];
        if centered {
            let mut width = 0.0;
            let mut previous = None;
            for id in text.chars().map(|character| font.glyph_id(character)) {
                width += scaled.h_advance(id);
                if let Some(last) = previous {
                    width += scaled.kern(last, id);
                }
                previous = Some(id);
            }
            x -= width * 0.5;
        }
        let mut previous = None;
        for character in text.chars() {
            let id = font.glyph_id(character);
            if let Some(last) = previous {
                x += scaled.kern(last, id);
            }
            let glyph = id.with_scale_and_position(size, point(x, at[1]));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    self.blend(
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                        color,
                        coverage,
                    )
                });
            }
            x += scaled.h_advance(id);
            previous = Some(id);
        }
    }

    fn sprite(&mut self, rect: [f32; 4], encoded: &str) -> Result<()> {
        let (image, info) = read_png(encoded)?;
        let channels = info.color_type.samples();
        let [x, y, width, height] = rect;
        let scale = (width / info.width as f32).min(height / info.height as f32);
        let draw_width = (info.width as f32 * scale).round() as i32;
        let draw_height = (info.height as f32 * scale).round() as i32;
        let left = (x + (width - draw_width as f32) / 2.0).round() as i32;
        let top = (y + (height - draw_height as f32) / 2.0).round() as i32;
        for y in 0.max(-top)..draw_height.min(self.height as i32 - top) {
            for x in 0.max(-left)..draw_width.min(self.width as i32 - left) {
                let source_x = (x as f32 / scale) as usize;
                let source_y = (y as f32 / scale) as usize;
                let offset = (source_y.min(info.height as usize - 1) * info.width as usize
                    + source_x.min(info.width as usize - 1))
                    * channels;
                let sample = &image[offset..offset + channels];
                let color = match sample {
                    [gray] => [*gray, *gray, *gray, 255],
                    [gray, alpha] => [*gray, *gray, *gray, *alpha],
                    [r, g, b] => [*r, *g, *b, 255],
                    [r, g, b, a] => [*r, *g, *b, *a],
                    _ => return Err("Unsupported PNG sprite color model".into()),
                };
                self.blend(left + x, top + y, color, 1.0);
            }
        }
        Ok(())
    }

    fn nine_patch(&mut self, rect: [f32; 4], edge_scale: f32, patch: &Patch) -> Result<()> {
        let (image, info) = read_png(&patch.png_base64)?;
        if [info.width, info.height] != patch.size {
            return Err("Invalid nine-patch source image".into());
        }
        let channels = info.color_type.samples();
        let [left, top, width, height] = rect.map(|value| value.round() as i32);
        let [west, north, east, south] = patch.insets;
        let horizontal = PatchAxis::new(width, info.width, [west, east], edge_scale);
        let vertical = PatchAxis::new(height, info.height, [north, south], edge_scale);
        for y in 0.max(-top)..height.min(self.height as i32 - top) {
            let row = vertical.sample(y) * info.width as usize;
            for x in 0.max(-left)..width.min(self.width as i32 - left) {
                let offset = (row + horizontal.sample(x)) * channels;
                let color = match &image[offset..offset + channels] {
                    [gray] => [*gray, *gray, *gray, 255],
                    [gray, alpha] => [*gray, *gray, *gray, *alpha],
                    [r, g, b] => [*r, *g, *b, 255],
                    [r, g, b, a] => [*r, *g, *b, *a],
                    _ => return Err("Unsupported nine-patch color model".into()),
                };
                self.blend(left + x, top + y, color, 1.0);
            }
        }
        Ok(())
    }
}

struct PatchAxis {
    source: f32,
    destination: f32,
    start: f32,
    end: f32,
    scale: f32,
}

impl PatchAxis {
    fn new(destination: i32, source: u32, [start, end]: [u32; 2], scale: f32) -> Self {
        let scale = scale.min(destination as f32 / (start + end) as f32);
        Self {
            source: source as f32,
            destination: destination as f32,
            start: start as f32,
            end: end as f32,
            scale,
        }
    }

    fn sample(&self, coordinate: i32) -> usize {
        let at = coordinate as f32 + 0.5;
        let left = self.start * self.scale;
        let right = self.destination - self.end * self.scale;
        let source = if at < left {
            at / self.scale
        } else if at >= right {
            self.source - (self.destination - at) / self.scale
        } else {
            self.start + (at - left) / (right - left) * (self.source - self.start - self.end)
        };
        source.clamp(0.0, self.source - 1.0) as usize
    }
}

fn read_png(encoded: &str) -> Result<(Vec<u8>, png::OutputInfo)> {
    let bytes = decode_base64(encoded)?;
    let mut decoder = png::Decoder::new(bytes.as_slice());
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut image = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut image)?;
    Ok((image, info))
}

pub(crate) fn png_size(encoded: &str) -> Result<[u32; 2]> {
    let bytes = decode_base64(encoded)?;
    let reader = png::Decoder::new(bytes.as_slice()).read_info()?;
    Ok([reader.info().width, reader.info().height])
}

pub(crate) fn check_font(bytes: &[u8]) -> Result<()> {
    ab_glyph::FontRef::try_from_slice(bytes).map_err(|_| "Invalid or unsupported font file")?;
    Ok(())
}

fn load_font(source: Option<&WorkspaceFont>) -> Result<FontArc> {
    let bytes = resolve_font(source)?.0;
    let mut font = FontVec::try_from_vec(bytes).map_err(|_| "Invalid or unsupported font file")?;
    if let Some(weight) = source.and_then(|font| font.weight) {
        font.set_variation(b"wght", f32::from(weight));
    }
    Ok(FontArc::new(font))
}

pub(crate) fn resolve_font(source: Option<&WorkspaceFont>) -> Result<(Vec<u8>, Option<PathBuf>)> {
    if let Some(data) = source.and_then(|font| font.data.as_deref()) {
        let bytes = decode_base64(data)?;
        check_font(&bytes)?;
        return Ok((bytes, None));
    }
    let defaults = vec![
        "segoeui.ttf".to_owned(),
        "DejaVuSans.ttf".to_owned(),
        "Arial.ttf".to_owned(),
    ];
    let names = source.map_or(defaults.as_slice(), |font| font.local_names.as_slice());
    let mut directories = Vec::new();
    if let Some(windows) = env::var_os("WINDIR") {
        directories.push(PathBuf::from(windows).join("Fonts"));
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
    }
    directories.extend(
        [
            "C:/Windows/Fonts",
            "/usr/share/fonts/truetype/dejavu",
            "/usr/share/fonts/truetype/liberation2",
            "/System/Library/Fonts/Supplemental",
        ]
        .map(PathBuf::from),
    );

    names
        .iter()
        .flat_map(|name| {
            directories
                .iter()
                .map(move |directory| directory.join(name))
        })
        .find_map(|path| {
            let bytes = fs::read(&path).ok()?;
            check_font(&bytes).ok()?;
            Some((bytes, Some(path)))
        })
        .ok_or_else(|| format!("No configured font file found for {names:?}").into())
}

pub(crate) fn decode_base64(value: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in value.bytes().take_while(|byte| *byte != b'=') {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("Invalid base64 sprite".into()),
        };
        buffer = buffer << 6 | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Ok(bytes)
}
