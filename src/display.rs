use crate::types::{BBox, Bounded, Char, Curve, Edge, Line, Page, Point, Word};
use crate::table::{Table, TableFinder, TableSettings};
use crate::Result;
use font8x8::UnicodeFonts;
use image::{ImageBuffer, ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_RESOLUTION: f64 = 72.0;

#[derive(Debug, Clone, Copy)]
struct ImageRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl ImageRect {
    fn at(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            width: 0,
            height: 0,
        }
    }

    fn of_size(self, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..self
        }
    }

    fn left(&self) -> i32 {
        self.x
    }

    fn top(&self) -> i32 {
        self.y
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}

fn put_pixel_clipped(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 {
        let (x, y) = (x as u32, y as u32);
        if x < image.width() && y < image.height() {
            image.put_pixel(x, y, color);
        }
    }
}

fn draw_filled_rect_mut(image: &mut RgbaImage, rect: ImageRect, color: Rgba<u8>) {
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x as i64 + rect.width as i64)
        .clamp(0, image.width() as i64) as u32;
    let y1 = (rect.y as i64 + rect.height as i64)
        .clamp(0, image.height() as i64) as u32;

    for y in y0..y1 {
        for x in x0..x1 {
            image.put_pixel(x, y, color);
        }
    }
}

fn draw_hollow_rect_mut(image: &mut RgbaImage, rect: ImageRect, color: Rgba<u8>) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let right = rect.x + rect.width as i32 - 1;
    let bottom = rect.y + rect.height as i32 - 1;
    for x in rect.x..=right {
        put_pixel_clipped(image, x, rect.y, color);
        put_pixel_clipped(image, x, bottom, color);
    }
    for y in rect.y..=bottom {
        put_pixel_clipped(image, rect.x, y, color);
        put_pixel_clipped(image, right, y, color);
    }
}

fn draw_line_segment_mut(image: &mut RgbaImage, start: (f32, f32), end: (f32, f32), color: Rgba<u8>) {
    let (x0, y0) = start;
    let (x1, y1) = end;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).ceil() as i32;
    if steps <= 0 {
        put_pixel_clipped(image, x0.round() as i32, y0.round() as i32, color);
        return;
    }

    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = x0 + dx * t;
        let y = y0 + dy * t;
        put_pixel_clipped(image, x.round() as i32, y.round() as i32, color);
    }
}

fn draw_filled_circle_mut(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let radius = radius.max(1);
    let radius_sq = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius_sq {
                put_pixel_clipped(image, center.0 + dx, center.1 + dy, color);
            }
        }
    }
}

fn draw_hollow_circle_mut(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let radius = radius.max(1);
    let radius_sq = radius * radius;
    let inner_sq = (radius - 1) * (radius - 1);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let dist = dx * dx + dy * dy;
            if dist <= radius_sq && dist >= inner_sq {
                put_pixel_clipped(image, center.0 + dx, center.1 + dy, color);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_rgba(self) -> Rgba<u8> {
        Rgba([self.r, self.g, self.b, self.a])
    }
}

impl Default for RgbaColor {
    fn default() -> Self {
        Self::new(0, 0, 0, 255)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RenderOptions {
    pub resolution: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub antialias: bool,
    pub force_mediabox: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            resolution: Some(DEFAULT_RESOLUTION),
            width: None,
            height: None,
            antialias: false,
            force_mediabox: false,
        }
    }
}

pub trait HasBBox {
    fn bbox(&self) -> BBox;
}

impl<T> HasBBox for T
where
    T: Bounded,
{
    fn bbox(&self) -> BBox {
        Bounded::bbox(self)
    }
}

impl HasBBox for BBox {
    fn bbox(&self) -> BBox {
        *self
    }
}

impl HasBBox for (f64, f64, f64, f64) {
    fn bbox(&self) -> BBox {
        BBox::new(self.0, self.1, self.2, self.3)
    }
}

impl HasBBox for Table {
    fn bbox(&self) -> BBox {
        self.bbox
    }
}

pub trait HasCenter {
    fn center(&self) -> Point;
}

impl<T> HasCenter for T
where
    T: HasBBox,
{
    fn center(&self) -> Point {
        self.bbox().center()
    }
}

pub trait HasLineSegments {
    fn line_segments(&self) -> Vec<(Point, Point)>;
}

impl HasLineSegments for Line {
    fn line_segments(&self) -> Vec<(Point, Point)> {
        self.pts
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect::<Vec<_>>()
    }
}

impl HasLineSegments for Edge {
    fn line_segments(&self) -> Vec<(Point, Point)> {
        vec![(Point::new(self.x0, self.top), Point::new(self.x1, self.bottom))]
    }
}

impl HasLineSegments for Curve {
    fn line_segments(&self) -> Vec<(Point, Point)> {
        self.pts
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect::<Vec<_>>()
    }
}

impl HasLineSegments for (Point, Point) {
    fn line_segments(&self) -> Vec<(Point, Point)> {
        vec![*self]
    }
}

impl HasLineSegments for ((f64, f64), (f64, f64)) {
    fn line_segments(&self) -> Vec<(Point, Point)> {
        vec![(Point::new((self.0).0, (self.0).1), Point::new((self.1).0, (self.1).1))]
    }
}

#[derive(Debug, Clone)]
pub struct PageImage {
    pub page: Page,
    pub resolution: f64,
    pub antialias: bool,
    pub force_mediabox: bool,
    pub bbox: BBox,
    pub original: RgbaImage,
    pub annotated: RgbaImage,
}

impl PageImage {
    pub fn new(page: &Page, options: RenderOptions) -> Result<Self> {
        let set_count = [options.resolution.is_some(), options.width.is_some(), options.height.is_some()]
            .into_iter()
            .filter(|item| *item)
            .count();
        if set_count > 1 {
            return Err(crate::Error::Message(
                "pass at most one of resolution, width, or height".to_string(),
            ));
        }

        let bbox = if page.bbox != page.mediabox {
            page.bbox
        } else if options.force_mediabox {
            page.mediabox
        } else {
            page.cropbox
        };

        let resolution = if let Some(resolution) = options.resolution {
            resolution
        } else if let Some(width) = options.width {
            DEFAULT_RESOLUTION * (width / bbox.width())
        } else if let Some(height) = options.height {
            DEFAULT_RESOLUTION * (height / bbox.height())
        } else {
            DEFAULT_RESOLUTION
        };

        let scale = resolution / DEFAULT_RESOLUTION;
        let width_px = ((bbox.width() * scale).round() as i64).max(1) as u32;
        let height_px = ((bbox.height() * scale).round() as i64).max(1) as u32;
        let mut image = ImageBuffer::from_pixel(width_px, height_px, Rgba([255, 255, 255, 255]));

        let mut page_image = Self {
            page: page.clone(),
            resolution,
            antialias: options.antialias,
            force_mediabox: options.force_mediabox,
            bbox,
            original: image.clone(),
            annotated: image.clone(),
        };
        page_image.render_page_content(&mut image);
        page_image.original = image.clone();
        page_image.annotated = image;
        Ok(page_image)
    }

    pub fn width(&self) -> u32 {
        self.annotated.width()
    }

    pub fn height(&self) -> u32 {
        self.annotated.height()
    }

    pub fn reset(&mut self) -> &mut Self {
        self.annotated = self.original.clone();
        self
    }

    pub fn copy(&self) -> Self {
        self.clone()
    }

    pub fn save<P: AsRef<Path>>(
        &self,
        dest: P,
        format: Option<ImageFormat>,
        _quantize: bool,
        _colors: u16,
        _bits: u8,
    ) -> Result<()> {
        if let Some(format) = format {
            self.annotated.save_with_format(dest, format)?;
        } else {
            self.annotated.save(dest)?;
        }
        Ok(())
    }

    pub fn show(&self) -> Result<PathBuf> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| crate::Error::Message(err.to_string()))?
            .as_millis();
        let path = std::env::temp_dir().join(format!("pdfsink-rs-page-{millis}.png"));
        self.save(&path, Some(ImageFormat::Png), false, 256, 8)?;

        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(&path).spawn();
        #[cfg(target_os = "linux")]
        let _ = Command::new("xdg-open").arg(&path).spawn();
        #[cfg(target_os = "windows")]
        let _ = Command::new("cmd").args(["/C", "start", path.to_string_lossy().as_ref()]).spawn();

        Ok(path)
    }

    pub fn draw_line<T: HasLineSegments>(&mut self, item: &T, stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        let color = stroke.to_rgba();
        for (start, end) in item.line_segments() {
            let (x0, y0) = self.project_point(start);
            let (x1, y1) = self.project_point(end);
            let offset = (stroke_width.max(1) as i32 - 1) / 2;
            for dx in -offset..=offset {
                for dy in -offset..=offset {
                    draw_line_segment_mut(
                        &mut self.annotated,
                        (x0 + dx as f32, y0 + dy as f32),
                        (x1 + dx as f32, y1 + dy as f32),
                        color,
                    );
                }
            }
        }
        self
    }

    pub fn draw_lines<T: HasLineSegments>(&mut self, items: &[T], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        for item in items {
            self.draw_line(item, stroke, stroke_width);
        }
        self
    }

    pub fn draw_vline(&mut self, location: f64, stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        self.draw_line(
            &(
                Point::new(location, self.bbox.top),
                Point::new(location, self.bbox.bottom),
            ),
            stroke,
            stroke_width,
        )
    }

    pub fn draw_vlines(&mut self, locations: &[f64], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        for location in locations {
            self.draw_vline(*location, stroke, stroke_width);
        }
        self
    }

    pub fn draw_hline(&mut self, location: f64, stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        self.draw_line(
            &(
                Point::new(self.bbox.x0, location),
                Point::new(self.bbox.x1, location),
            ),
            stroke,
            stroke_width,
        )
    }

    pub fn draw_hlines(&mut self, locations: &[f64], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        for location in locations {
            self.draw_hline(*location, stroke, stroke_width);
        }
        self
    }

    pub fn draw_rect<T: HasBBox>(
        &mut self,
        item: &T,
        fill: Option<RgbaColor>,
        stroke: Option<RgbaColor>,
        stroke_width: u32,
    ) -> &mut Self {
        let bbox = item.bbox();
        let rect = self.project_rect(bbox);
        if rect.width() == 0 || rect.height() == 0 {
            return self;
        }
        if let Some(fill) = fill {
            draw_filled_rect_mut(&mut self.annotated, rect, fill.to_rgba());
        }
        if let Some(stroke) = stroke {
            for inset in 0..stroke_width.max(1) {
                let x = rect.left() + inset as i32;
                let y = rect.top() + inset as i32;
                let w = rect.width().saturating_sub(inset.saturating_mul(2));
                let h = rect.height().saturating_sub(inset.saturating_mul(2));
                if w == 0 || h == 0 {
                    continue;
                }
                let inset_rect = ImageRect::at(x, y).of_size(w, h);
                draw_hollow_rect_mut(&mut self.annotated, inset_rect, stroke.to_rgba());
            }
        }
        self
    }

    pub fn draw_rects<T: HasBBox>(
        &mut self,
        items: &[T],
        fill: Option<RgbaColor>,
        stroke: Option<RgbaColor>,
        stroke_width: u32,
    ) -> &mut Self {
        for item in items {
            self.draw_rect(item, fill, stroke, stroke_width);
        }
        self
    }

    pub fn draw_circle<T: HasCenter>(
        &mut self,
        item: &T,
        radius: i32,
        fill: Option<RgbaColor>,
        stroke: Option<RgbaColor>,
    ) -> &mut Self {
        let center = item.center();
        let (x, y) = self.project_point(center);
        let center = (x.round() as i32, y.round() as i32);
        if let Some(fill) = fill {
            draw_filled_circle_mut(&mut self.annotated, center, radius.max(1), fill.to_rgba());
        }
        if let Some(stroke) = stroke {
            draw_hollow_circle_mut(&mut self.annotated, center, radius.max(1), stroke.to_rgba());
        }
        self
    }

    pub fn draw_circles<T: HasCenter>(
        &mut self,
        items: &[T],
        radius: i32,
        fill: Option<RgbaColor>,
        stroke: Option<RgbaColor>,
    ) -> &mut Self {
        for item in items {
            self.draw_circle(item, radius, fill, stroke);
        }
        self
    }

    pub fn outline_words(&mut self, words: &[Word], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        self.draw_rects(words, None, Some(stroke), stroke_width)
    }

    pub fn outline_chars(&mut self, chars: &[Char], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        self.draw_rects(chars, None, Some(stroke), stroke_width)
    }

    pub fn outline_edges(&mut self, edges: &[Edge], stroke: RgbaColor, stroke_width: u32) -> &mut Self {
        self.draw_lines(edges, stroke, stroke_width)
    }

    pub fn outline_tables(&mut self, tables: &[Table], fill: Option<RgbaColor>, stroke: Option<RgbaColor>, stroke_width: u32) -> &mut Self {
        for table in tables {
            for cell in &table.cells {
                self.draw_rect(cell, fill, stroke, stroke_width);
            }
        }
        self
    }

    pub fn debug_tablefinder(&mut self, table_settings: Option<TableSettings>) -> Result<&mut Self> {
        let finder = if let Some(settings) = table_settings {
            self.page.debug_tablefinder(settings)?
        } else {
            self.page.debug_tablefinder(TableSettings::default())?
        };
        self.overlay_tablefinder(&finder);
        Ok(self)
    }

    pub fn overlay_tablefinder(&mut self, finder: &TableFinder) -> &mut Self {
        let red = RgbaColor::new(255, 0, 0, 255);
        let light_blue = RgbaColor::new(173, 216, 230, 96);
        for edge in &finder.edges {
            self.draw_line(edge, red, 1);
        }
        for intersection in finder.intersections.keys() {
            let point = Point::new(intersection.0.into_inner(), intersection.1.into_inner());
            self.draw_circle(&point, 4, Some(red), Some(red));
        }
        for cell in &finder.cells {
            self.draw_rect(cell, Some(light_blue), Some(red), 1);
        }
        self
    }

    fn render_page_content(&mut self, image: &mut RgbaImage) {
        for rect in &self.page.rects {
            let fill = if rect.fill {
                Some(RgbaColor::new(235, 235, 235, 255).to_rgba())
            } else {
                None
            };
            let stroke = if rect.stroke {
                Some(RgbaColor::default().to_rgba())
            } else {
                None
            };
            let projected = self.project_rect(Bounded::bbox(rect));
            if let Some(fill) = fill {
                draw_filled_rect_mut(image, projected, fill);
            }
            if let Some(stroke) = stroke {
                draw_hollow_rect_mut(image, projected, stroke);
            }
        }

        for line in &self.page.lines {
            self.render_segment(image, line);
        }
        for curve in &self.page.curves {
            self.render_segment(image, curve);
        }
        for image_obj in &self.page.images {
            let rect = self.project_rect(Bounded::bbox(image_obj));
            draw_hollow_rect_mut(image, rect, RgbaColor::new(120, 120, 120, 255).to_rgba());
        }
        for ch in &self.page.chars {
            self.render_char(image, ch);
        }
    }

    fn render_segment<T: HasLineSegments>(&self, image: &mut RgbaImage, item: &T) {
        for (start, end) in item.line_segments() {
            let (x0, y0) = self.project_point(start);
            let (x1, y1) = self.project_point(end);
            draw_line_segment_mut(image, (x0, y0), (x1, y1), RgbaColor::default().to_rgba());
        }
    }

    fn render_char(&self, image: &mut RgbaImage, ch: &Char) {
        let Some(letter) = ch.text.chars().next() else {
            return;
        };
        if letter.is_whitespace() {
            return;
        }
        let bbox = Bounded::bbox(ch);
        let left = ((bbox.x0 - self.bbox.x0) * self.scale()).floor().max(0.0) as u32;
        let top = ((bbox.top - self.bbox.top) * self.scale()).floor().max(0.0) as u32;
        let width = ((bbox.width() * self.scale()).ceil().max(1.0)) as u32;
        let height = ((bbox.height() * self.scale()).ceil().max(1.0)) as u32;

        if let Some(glyph) = font8x8::BASIC_FONTS.get(letter) {
            for (row_idx, row) in glyph.iter().enumerate() {
                for col_idx in 0..8u32 {
                    if ((*row >> col_idx) & 1) == 1 {
                        let x_start = left + (col_idx * width / 8);
                        let x_end = left + (((col_idx + 1) * width + 7) / 8).max(1);
                        let y_start = top + (row_idx as u32 * height / 8);
                        let y_end = top + ((((row_idx as u32) + 1) * height + 7) / 8).max(1);
                        for y in y_start..y_end.min(image.height()) {
                            for x in x_start..x_end.min(image.width()) {
                                image.put_pixel(x, y, RgbaColor::default().to_rgba());
                            }
                        }
                    }
                }
            }
        } else {
            let rect = self.project_rect(bbox);
            draw_hollow_rect_mut(image, rect, RgbaColor::default().to_rgba());
        }
    }

    fn scale(&self) -> f64 {
        self.resolution / DEFAULT_RESOLUTION
    }

    fn project_point(&self, point: Point) -> (f32, f32) {
        (
            ((point.x - self.bbox.x0) * self.scale()) as f32,
            ((point.y - self.bbox.top) * self.scale()) as f32,
        )
    }

    fn project_rect(&self, bbox: BBox) -> ImageRect {
        let x = ((bbox.x0 - self.bbox.x0) * self.scale()).floor() as i32;
        let y = ((bbox.top - self.bbox.top) * self.scale()).floor() as i32;
        let width = ((bbox.width() * self.scale()).ceil() as i64).max(1) as u32;
        let height = ((bbox.height() * self.scale()).ceil() as i64).max(1) as u32;
        ImageRect::at(x, y).of_size(width, height)
    }
}

impl HasCenter for Point {
    fn center(&self) -> Point {
        *self
    }
}

impl Page {
    pub fn to_image(
        &self,
        resolution: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
        antialias: bool,
        force_mediabox: bool,
    ) -> Result<PageImage> {
        PageImage::new(
            self,
            RenderOptions {
                resolution,
                width,
                height,
                antialias,
                force_mediabox,
            },
        )
    }
}
