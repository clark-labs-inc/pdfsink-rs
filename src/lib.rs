mod clustering;
mod container_api;
mod display;
mod error;
mod geometry;
mod layout;
mod ops;
mod parse;
mod path_geometry;
mod table;
mod text;
mod types;

pub use display::{HasBBox, HasCenter, HasLineSegments, PageImage, RenderOptions, RgbaColor};
pub use error::{Error, Result};
pub use parse::{open_pdf, open_pdf_bytes, open_pdf_bytes_pages, open_pdf_pages};
pub use table::{table_rows_to_csv, CellGroup, ExplicitLine, Table, TableFinder, TableSettings, TableStrategy};
pub use text::{
    chars_to_textmap, dedupe_chars, extract_text, extract_text_lines, extract_text_simple,
    extract_words, DedupeOptions, SearchOptions, TextMap, TextOptions, WordExtractor, WordMap,
};
pub use types::{
    Annotation, BBox, Char, Curve, Direction, Edge, Hyperlink, ImageObject, JsonMap, LayoutObject,
    Line, ObjectCounts, Orientation, Page, PageLayout, PageObjectRef, PathCommand, PdfDocument,
    Point, RectObject, SearchMatch, StructureElement, TextLine, Word,
};

pub type PDF = PdfDocument;

use geometry::{crop_objects, outside_objects, test_proposed_bbox, within_objects};

impl PdfDocument {
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        open_pdf(path)
    }

    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Self> {
        open_pdf_bytes(bytes)
    }

    pub fn open_pages<P: AsRef<std::path::Path>>(
        path: P,
        page_numbers: &[usize],
    ) -> Result<Self> {
        open_pdf_pages(path, page_numbers)
    }

    pub fn from_bytes_pages<B: AsRef<[u8]>>(
        bytes: B,
        page_numbers: &[usize],
    ) -> Result<Self> {
        open_pdf_bytes_pages(bytes, page_numbers)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn page(&self, page_number: usize) -> Result<&Page> {
        if page_number == 0 || page_number > self.pages.len() {
            return Err(Error::InvalidPage { page_number });
        }
        Ok(&self.pages[page_number - 1])
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }
}

impl Page {
    pub fn object_counts(&self) -> ObjectCounts {
        ObjectCounts {
            chars: self.chars.len(),
            lines: self.lines.len(),
            rects: self.rects.len(),
            curves: self.curves.len(),
            images: self.images.len(),
            annots: self.annots.len(),
            hyperlinks: self.hyperlinks.len(),
        }
    }

    pub fn extract_text(&self) -> String {
        extract_text(&self.chars, &self.default_text_options())
    }

    pub fn extract_text_with_options(&self, options: &TextOptions) -> String {
        extract_text(&self.chars, options)
    }

    pub fn extract_text_simple(&self) -> String {
        extract_text_simple(&self.chars, 3.0, 3.0)
    }

    pub fn extract_text_simple_with_tolerance(&self, x_tolerance: f64, y_tolerance: f64) -> String {
        extract_text_simple(&self.chars, x_tolerance, y_tolerance)
    }

    pub fn extract_words(&self) -> Vec<Word> {
        extract_words(&self.chars, &self.default_text_options(), false)
    }

    pub fn extract_words_with_options(&self, options: &TextOptions, return_chars: bool) -> Vec<Word> {
        extract_words(&self.chars, options, return_chars)
    }

    pub fn extract_text_lines(&self, strip: bool, return_chars: bool) -> Vec<TextLine> {
        extract_text_lines(&self.chars, &self.default_text_options(), strip, return_chars)
    }

    pub fn search(&self, pattern: &str) -> Result<Vec<SearchMatch>> {
        let textmap = chars_to_textmap(&self.chars, &self.default_text_options());
        textmap.search(pattern, &SearchOptions::default())
    }

    pub fn search_with_options(&self, pattern: &str, options: &SearchOptions, text_options: &TextOptions) -> Result<Vec<SearchMatch>> {
        let textmap = chars_to_textmap(&self.chars, text_options);
        textmap.search(pattern, options)
    }

    pub fn crop(&self, bbox: BBox, relative: bool, strict: bool) -> Result<Self> {
        self.crop_inner(bbox, relative, strict, CropMode::Crop)
    }

    pub fn within_bbox(&self, bbox: BBox, relative: bool, strict: bool) -> Result<Self> {
        self.crop_inner(bbox, relative, strict, CropMode::Within)
    }

    pub fn outside_bbox(&self, bbox: BBox, relative: bool, strict: bool) -> Result<Self> {
        self.crop_inner(bbox, relative, strict, CropMode::Outside)
    }

    pub fn filter<F>(&self, mut predicate: F) -> Self
    where
        F: FnMut(PageObjectRef<'_>) -> bool,
    {
        let mut page = self.clone();
        page.chars = self
            .chars
            .iter()
            .filter(|item| predicate(PageObjectRef::Char(item)))
            .cloned()
            .collect();
        page.lines = self
            .lines
            .iter()
            .filter(|item| predicate(PageObjectRef::Line(item)))
            .cloned()
            .collect();
        page.rects = self
            .rects
            .iter()
            .filter(|item| predicate(PageObjectRef::Rect(item)))
            .cloned()
            .collect();
        page.curves = self
            .curves
            .iter()
            .filter(|item| predicate(PageObjectRef::Curve(item)))
            .cloned()
            .collect();
        page.images = self
            .images
            .iter()
            .filter(|item| predicate(PageObjectRef::Image(item)))
            .cloned()
            .collect();
        page.annots = self
            .annots
            .iter()
            .filter(|item| predicate(PageObjectRef::Annot(item)))
            .cloned()
            .collect();
        page.hyperlinks = self
            .hyperlinks
            .iter()
            .filter(|item| predicate(PageObjectRef::Hyperlink(item)))
            .cloned()
            .collect();
        page.is_original = false;
        page
    }

    pub fn dedupe_chars(&self, options: &DedupeOptions) -> Self {
        let mut page = self.clone();
        page.chars = dedupe_chars(&self.chars, options);
        page.is_original = false;
        page
    }

    pub fn debug_tablefinder(&self, settings: TableSettings) -> Result<TableFinder> {
        TableFinder::new(self, settings)
    }

    pub fn find_tables(&self, settings: TableSettings) -> Result<Vec<Table>> {
        Ok(TableFinder::new(self, settings)?.tables)
    }

    pub fn find_table(&self, settings: TableSettings) -> Result<Option<Table>> {
        let mut tables = self.find_tables(settings)?;
        if tables.is_empty() {
            return Ok(None);
        }
        tables.sort_by(|a, b| {
            b.cells
                .len()
                .cmp(&a.cells.len())
                .then_with(|| a.bbox.top.total_cmp(&b.bbox.top))
                .then_with(|| a.bbox.x0.total_cmp(&b.bbox.x0))
        });
        Ok(tables.into_iter().next())
    }

    pub fn extract_tables(&self, settings: TableSettings) -> Result<Vec<Vec<Vec<Option<String>>>>> {
        let tables = self.find_tables(settings.clone())?;
        Ok(tables
            .iter()
            .map(|table| table.extract(self, &settings.text_options))
            .collect())
    }

    pub fn extract_table(&self, settings: TableSettings) -> Result<Option<Vec<Vec<Option<String>>>>> {
        let Some(table) = self.find_table(settings.clone())? else {
            return Ok(None);
        };
        Ok(Some(table.extract(self, &settings.text_options)))
    }

    pub fn to_debug_svg(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {:.3} {:.3}" width="{:.3}" height="{:.3}">"#,
            self.width, self.height, self.width, self.height
        ));
        out.push_str(r#"<rect x="0" y="0" width="100%" height="100%" fill="white" stroke="black"/>"#);

        for line in &self.lines {
            if line.pts.len() >= 2 {
                out.push_str(&format!(
                    r#"<line x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}" stroke="black" stroke-width="1"/>"#,
                    line.pts[0].x, line.pts[0].y, line.pts[1].x, line.pts[1].y
                ));
            }
        }

        for rect in &self.rects {
            out.push_str(&format!(
                r#"<rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="none" stroke="black" stroke-width="1"/>"#,
                rect.x0, rect.top, rect.width, rect.height
            ));
        }

        for curve in &self.curves {
            if let Some(first) = curve.pts.first() {
                let mut d = format!("M {:.3} {:.3}", first.x, first.y);
                for point in curve.pts.iter().skip(1) {
                    d.push_str(&format!(" L {:.3} {:.3}", point.x, point.y));
                }
                out.push_str(&format!(
                    r#"<path d="{}" fill="none" stroke="black" stroke-width="1"/>"#,
                    d
                ));
            }
        }

        for image in &self.images {
            out.push_str(&format!(
                r#"<rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="none" stroke="black" stroke-dasharray="4 2"/>"#,
                image.x0, image.top, image.width, image.height
            ));
        }

        for link in &self.hyperlinks {
            out.push_str(&format!(
                r#"<rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="none" stroke="black" stroke-dasharray="2 2"/>"#,
                link.x0, link.top, link.width, link.height
            ));
        }

        for ch in &self.chars {
            let x = ch.x0;
            let y = ch.bottom;
            let escaped = html_escape(&ch.text);
            out.push_str(&format!(
                r#"<text x="{:.3}" y="{:.3}" font-size="10">{}</text>"#,
                x, y, escaped
            ));
        }

        out.push_str("</svg>");
        out
    }

    fn crop_inner(&self, bbox: BBox, relative: bool, strict: bool, mode: CropMode) -> Result<Self> {
        let proposed = if relative {
            bbox.translate(self.bbox.x0, self.bbox.top)
        } else {
            bbox
        };

        if strict {
            test_proposed_bbox(proposed, self.bbox)?;
        }

        let mut page = self.clone();
        page.chars = match mode {
            CropMode::Crop => crop_objects(&self.chars, proposed, self.height),
            CropMode::Within => within_objects(&self.chars, proposed),
            CropMode::Outside => outside_objects(&self.chars, proposed),
        };
        page.lines = match mode {
            CropMode::Crop => crop_objects(&self.lines, proposed, self.height),
            CropMode::Within => within_objects(&self.lines, proposed),
            CropMode::Outside => outside_objects(&self.lines, proposed),
        };
        page.rects = match mode {
            CropMode::Crop => crop_objects(&self.rects, proposed, self.height),
            CropMode::Within => within_objects(&self.rects, proposed),
            CropMode::Outside => outside_objects(&self.rects, proposed),
        };
        page.curves = match mode {
            CropMode::Crop => crop_objects(&self.curves, proposed, self.height),
            CropMode::Within => within_objects(&self.curves, proposed),
            CropMode::Outside => outside_objects(&self.curves, proposed),
        };
        page.images = match mode {
            CropMode::Crop => crop_objects(&self.images, proposed, self.height),
            CropMode::Within => within_objects(&self.images, proposed),
            CropMode::Outside => outside_objects(&self.images, proposed),
        };
        page.annots = match mode {
            CropMode::Crop => crop_objects(&self.annots, proposed, self.height),
            CropMode::Within => within_objects(&self.annots, proposed),
            CropMode::Outside => outside_objects(&self.annots, proposed),
        };
        page.hyperlinks = match mode {
            CropMode::Crop => crop_objects(&self.hyperlinks, proposed, self.height),
            CropMode::Within => within_objects(&self.hyperlinks, proposed),
            CropMode::Outside => outside_objects(&self.hyperlinks, proposed),
        };

        page.bbox = match mode {
            CropMode::Outside => self.bbox,
            CropMode::Crop | CropMode::Within => proposed,
        };
        page.is_original = false;

        Ok(page)
    }

    pub(crate) fn default_text_options(&self) -> TextOptions {
        let mut options = TextOptions::default();
        options.layout_bbox = Some(self.bbox);
        options.layout_width = Some(self.width);
        options.layout_height = Some(self.height);
        options
    }
}

#[derive(Clone, Copy)]
enum CropMode {
    Crop,
    Within,
    Outside,
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
