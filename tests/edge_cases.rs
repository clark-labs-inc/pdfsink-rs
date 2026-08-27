//! Comprehensive edge-case and coverage tests for pdfsink-rs.
//! Covers error handling, boundary conditions, all API surfaces,
//! custom options, and cross-feature interactions.

use pdfsink_rs::*;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// ---------------------------------------------------------------------------
// 1. ERROR HANDLING
// ---------------------------------------------------------------------------

#[test]
fn open_nonexistent_file_returns_error() {
    let result = PdfDocument::open("/nonexistent/path/fake.pdf");
    assert!(result.is_err());
}

#[test]
fn open_non_pdf_file_returns_error() {
    // Cargo.toml exists but is not a PDF
    let result = PdfDocument::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    );
    assert!(result.is_err());
}

#[test]
fn open_pdf_from_bytes_matches_path_loading() {
    let path = fixture("simple_text.pdf");
    let bytes = std::fs::read(&path).unwrap();

    let from_path = PdfDocument::open(&path).unwrap();
    let from_bytes = PdfDocument::from_bytes(bytes).unwrap();

    assert_eq!(from_bytes.len(), from_path.len());
    assert_eq!(
        from_bytes.page(1).unwrap().extract_text(),
        from_path.page(1).unwrap().extract_text()
    );
}

#[test]
fn open_selected_pages_parses_only_requested_source_pages() {
    let path = fixture("multipage.pdf");
    let selected = PdfDocument::open_pages(&path, &[2]).unwrap();

    assert_eq!(selected.len(), 1);
    let page = selected.page(1).unwrap();
    assert_eq!(page.page_number, 2);
    assert!(page.extract_text().contains("Page Two"));
    assert!(!page.extract_text().contains("Page One"));
}

#[test]
fn open_selected_pages_rejects_an_invalid_source_page() {
    let error = PdfDocument::open_pages(fixture("multipage.pdf"), &[3]).unwrap_err();
    match error {
        Error::InvalidPage { page_number } => assert_eq!(page_number, 3),
        other => panic!("expected InvalidPage, got {other:?}"),
    }
}

#[test]
fn open_invalid_bytes_returns_error() {
    let result = PdfDocument::from_bytes(b"this is not a valid pdf");
    assert!(result.is_err());
}

#[test]
fn page_zero_returns_invalid_page() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let result = pdf.page(0);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidPage { page_number } => assert_eq!(page_number, 0),
        other => panic!("expected InvalidPage, got {:?}", other),
    }
}

#[test]
fn page_out_of_range_returns_invalid_page() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let result = pdf.page(999);
    assert!(result.is_err());
}

#[test]
fn search_invalid_regex_returns_error() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = SearchOptions {
        regex: true,
        case_sensitive: true,
        main_group: 0,
        return_groups: false,
        return_chars: false,
    };
    let result = page.search_with_options("[invalid(", &opts, &TextOptions::default());
    assert!(result.is_err());
}

#[test]
fn crop_strict_outside_page_returns_error() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let bad_bbox = BBox::new(-100.0, -100.0, 9999.0, 9999.0);
    let result = page.crop(bad_bbox, false, true);
    assert!(result.is_err());
}

#[test]
fn crop_non_strict_outside_page_succeeds() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let big_bbox = BBox::new(-100.0, -100.0, 9999.0, 9999.0);
    let result = page.crop(big_bbox, false, false);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// 2. DOCUMENT / PAGE BASICS
// ---------------------------------------------------------------------------

#[test]
fn pdf_type_alias_works() {
    let _: PDF = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
}

#[test]
fn document_len_and_is_empty() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    assert_eq!(pdf.len(), 1);
    assert!(!pdf.is_empty());
}

#[test]
fn multipage_document_page_access() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    assert_eq!(pdf.len(), 2);
    assert_eq!(pdf.page(1).unwrap().page_number, 1);
    assert_eq!(pdf.page(2).unwrap().page_number, 2);
    assert!(pdf.page(3).is_err());
}

#[test]
fn pages_slice_returns_all() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    assert_eq!(pdf.pages().len(), 2);
}

#[test]
fn page_geometry_is_populated() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    assert!(page.width > 0.0);
    assert!(page.height > 0.0);
    assert!(page.bbox.is_valid());
    assert!(page.mediabox.is_valid());
    assert!(page.cropbox.is_valid());
    assert_eq!(page.doctop_offset, 0.0);
    assert!(page.is_original);
}

#[test]
fn page_boxes_mediabox_equals_cropbox_when_no_explicit_cropbox() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // simple_text.pdf has no explicit CropBox, so cropbox should default to mediabox
    assert_eq!(page.mediabox.x0, page.cropbox.x0);
    assert_eq!(page.mediabox.top, page.cropbox.top);
}

#[test]
fn metadata_is_extracted() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    // Test fixtures have metadata set
    assert!(!pdf.metadata.is_empty());
}

#[test]
fn object_counts_are_consistent() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let counts = page.object_counts();
    assert_eq!(counts.chars, page.chars.len());
    assert_eq!(counts.lines, page.lines.len());
    assert_eq!(counts.rects, page.rects.len());
    assert_eq!(counts.curves, page.curves.len());
    assert_eq!(counts.images, page.images.len());
    assert_eq!(counts.annots, page.annots.len());
    assert_eq!(counts.hyperlinks, page.hyperlinks.len());
}

// ---------------------------------------------------------------------------
// 3. TEXT EXTRACTION VARIANTS
// ---------------------------------------------------------------------------

#[test]
fn extract_text_returns_content() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let text = page.extract_text();
    assert!(text.contains("Hello"));
}

#[test]
fn extract_text_simple_returns_content() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let text = page.extract_text_simple();
    assert!(text.contains("Hello"));
}

#[test]
fn extract_text_simple_with_tolerance() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let tight = page.extract_text_simple_with_tolerance(0.5, 0.5);
    let loose = page.extract_text_simple_with_tolerance(20.0, 20.0);
    // Both should contain text, but may differ in whitespace
    assert!(!tight.is_empty());
    assert!(!loose.is_empty());
}

#[test]
fn extract_text_with_custom_options() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut opts = TextOptions::default();
    opts.x_tolerance = 1.0;
    opts.y_tolerance = 1.0;
    let text = page.extract_text_with_options(&opts);
    assert!(!text.is_empty());
}

#[test]
fn extract_text_with_layout_mode() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut opts = TextOptions::default();
    opts.layout = true;
    opts.layout_width = Some(page.width);
    opts.layout_height = Some(page.height);
    opts.layout_bbox = Some(page.bbox);
    let text = page.extract_text_with_options(&opts);
    assert!(!text.is_empty());
}

#[test]
fn extract_text_lines_basic() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.extract_text_lines(false, false);
    assert!(!lines.is_empty());
    assert!(!lines[0].text.is_empty());
    assert!(lines[0].chars.is_none()); // return_chars = false
}

#[test]
fn extract_text_lines_with_chars() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.extract_text_lines(false, true);
    assert!(!lines.is_empty());
    assert!(lines[0].chars.is_some()); // return_chars = true
    assert!(!lines[0].chars.as_ref().unwrap().is_empty());
}

#[test]
fn extract_text_lines_stripped() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.extract_text_lines(true, false);
    for line in &lines {
        assert_eq!(line.text, line.text.trim());
    }
}

#[test]
fn text_line_has_valid_bbox() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.extract_text_lines(false, false);
    for line in &lines {
        assert!(line.x0 <= line.x1);
        assert!(line.top <= line.bottom);
    }
}

// ---------------------------------------------------------------------------
// 4. WORD EXTRACTION
// ---------------------------------------------------------------------------

#[test]
fn extract_words_basic() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let words = page.extract_words();
    assert!(!words.is_empty());
    assert!(words.iter().any(|w| w.text == "Hello"));
}

#[test]
fn extract_words_with_options_return_chars() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let words = page.extract_words_with_options(&TextOptions::default(), true);
    assert!(!words.is_empty());
    let first = &words[0];
    assert!(first.chars.is_some());
    assert!(!first.chars.as_ref().unwrap().is_empty());
}

#[test]
fn word_has_valid_geometry() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    for word in page.extract_words() {
        assert!(word.x0 <= word.x1, "word '{}' has inverted x", word.text);
        assert!(word.top <= word.bottom, "word '{}' has inverted y", word.text);
        assert!(word.width >= 0.0);
        assert!(word.height >= 0.0);
    }
}

#[test]
fn extract_words_with_split_at_punctuation() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut opts = TextOptions::default();
    opts.split_at_punctuation = Some(".,-".to_string());
    let words = page.extract_words_with_options(&opts, false);
    assert!(!words.is_empty());
}

// ---------------------------------------------------------------------------
// 5. SEARCH
// ---------------------------------------------------------------------------

#[test]
fn search_literal() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let matches = page.search("Hello").unwrap();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].text, "Hello");
}

#[test]
fn search_no_results() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let matches = page.search("ZZZZNOTFOUND").unwrap();
    assert!(matches.is_empty());
}

#[test]
fn search_with_regex() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = SearchOptions {
        regex: true,
        case_sensitive: false,
        main_group: 0,
        return_groups: false,
        return_chars: false,
    };
    let matches = page
        .search_with_options(r"[Hh]ello", &opts, &TextOptions::default())
        .unwrap();
    assert!(!matches.is_empty());
}

#[test]
fn search_case_insensitive() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = SearchOptions {
        regex: false,
        case_sensitive: false,
        main_group: 0,
        return_groups: false,
        return_chars: false,
    };
    let matches = page
        .search_with_options("hello", &opts, &TextOptions::default())
        .unwrap();
    assert!(!matches.is_empty());
}

#[test]
fn search_with_groups() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = SearchOptions {
        regex: true,
        case_sensitive: true,
        main_group: 0,
        return_groups: true,
        return_chars: false,
    };
    let matches = page
        .search_with_options(r"(Hello)\s+(World)", &opts, &TextOptions::default())
        .unwrap();
    if !matches.is_empty() {
        let m = &matches[0];
        assert!(m.groups.is_some());
    }
}

#[test]
fn search_with_return_chars() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = SearchOptions {
        regex: false,
        case_sensitive: true,
        main_group: 0,
        return_groups: false,
        return_chars: true,
    };
    let matches = page
        .search_with_options("Hello", &opts, &TextOptions::default())
        .unwrap();
    assert!(!matches.is_empty());
    assert!(matches[0].chars.is_some());
}

#[test]
fn search_match_has_valid_bbox() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    for m in page.search("Hello").unwrap() {
        assert!(m.x0 <= m.x1);
        assert!(m.top <= m.bottom);
    }
}

// ---------------------------------------------------------------------------
// 6. CROPPING / SPATIAL FILTERING
// ---------------------------------------------------------------------------

#[test]
fn crop_reduces_content() {
    let pdf = PdfDocument::open(fixture("crop_regions.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let full_chars = page.chars.len();
    // Crop to left half
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let cropped = page.crop(half, false, false).unwrap();
    assert!(cropped.chars.len() < full_chars);
    assert!(!cropped.is_original);
}

#[test]
fn within_bbox_returns_subset() {
    let pdf = PdfDocument::open(fixture("crop_regions.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let full_chars = page.chars.len();
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let within = page.within_bbox(half, false, false).unwrap();
    assert!(within.chars.len() <= full_chars);
    assert!(!within.is_original);
}

#[test]
fn outside_bbox_returns_complement() {
    let pdf = PdfDocument::open(fixture("crop_regions.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let full_chars = page.chars.len();
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let outside = page.outside_bbox(half, false, false).unwrap();
    assert!(outside.chars.len() < full_chars);
    assert!(!outside.is_original);
}

#[test]
fn crop_relative_mode() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // Relative bbox: top-left quadrant relative to page bbox
    let rel = BBox::new(0.0, 0.0, page.width / 2.0, page.height / 2.0);
    let cropped = page.crop(rel, true, false).unwrap();
    assert!(!cropped.is_original);
}

#[test]
fn crop_preserves_object_types() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // Crop to full page — should keep everything
    let cropped = page.crop(page.bbox, false, false).unwrap();
    assert_eq!(cropped.chars.len(), page.chars.len());
}

// ---------------------------------------------------------------------------
// 7. FILTER
// ---------------------------------------------------------------------------

#[test]
fn filter_keeps_matching_objects() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let original_count = page.chars.len();
    // Keep all chars — count should stay the same
    let filtered = page.filter(|_| true);
    assert_eq!(filtered.chars.len(), original_count);
    assert!(!filtered.is_original);
}

#[test]
fn filter_removes_all() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let filtered = page.filter(|_| false);
    assert_eq!(filtered.chars.len(), 0);
    assert_eq!(filtered.lines.len(), 0);
    assert_eq!(filtered.rects.len(), 0);
}

#[test]
fn filter_by_object_type() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // Keep only chars
    let filtered = page.filter(|obj| matches!(obj, PageObjectRef::Char(_)));
    assert_eq!(filtered.chars.len(), page.chars.len());
    assert_eq!(filtered.lines.len(), 0);
    assert_eq!(filtered.rects.len(), 0);
    assert_eq!(filtered.curves.len(), 0);
    assert_eq!(filtered.images.len(), 0);
}

// ---------------------------------------------------------------------------
// 8. DEDUPLICATION
// ---------------------------------------------------------------------------

#[test]
fn dedupe_chars_reduces_duplicates() {
    let pdf = PdfDocument::open(fixture("rotated_and_duplicates.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let original = page.chars.len();
    let deduped = page.dedupe_chars(&DedupeOptions::default());
    assert!(deduped.chars.len() <= original);
    assert!(!deduped.is_original);
}

#[test]
fn dedupe_with_tight_tolerance() {
    let pdf = PdfDocument::open(fixture("rotated_and_duplicates.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let opts = DedupeOptions {
        tolerance: 0.01,
        extra_attrs: vec!["fontname".to_string()],
    };
    let deduped = page.dedupe_chars(&opts);
    // Very tight tolerance — fewer removals
    assert!(deduped.chars.len() >= page.dedupe_chars(&DedupeOptions::default()).chars.len());
}

// ---------------------------------------------------------------------------
// 9. TABLE EXTRACTION
// ---------------------------------------------------------------------------

#[test]
fn find_tables_with_lines_strategy() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let tables = page.find_tables(TableSettings::default()).unwrap();
    assert!(!tables.is_empty());
    assert!(tables[0].cells.len() > 0);
    assert!(tables[0].bbox.is_valid());
}

#[test]
fn extract_table_returns_grid() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let table = page.extract_table(TableSettings::default()).unwrap();
    assert!(table.is_some());
    let grid = table.unwrap();
    assert!(!grid.is_empty());
    assert!(!grid[0].is_empty());
}

#[test]
fn extract_tables_multiple() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let tables = page.extract_tables(TableSettings::default()).unwrap();
    assert!(!tables.is_empty());
}

#[test]
fn find_table_returns_largest() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let table = page.find_table(TableSettings::default()).unwrap();
    assert!(table.is_some());
}

#[test]
fn table_text_strategy() {
    let pdf = PdfDocument::open(fixture("table_text_only.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut settings = TableSettings::default();
    settings.vertical_strategy = TableStrategy::Text;
    settings.horizontal_strategy = TableStrategy::Text;
    let table = page.extract_table(settings).unwrap();
    assert!(table.is_some());
}

#[test]
fn no_table_returns_none() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let table = page.find_table(TableSettings::default()).unwrap();
    assert!(table.is_none());
}

#[test]
fn debug_tablefinder_returns_diagnostic() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let finder = page.debug_tablefinder(TableSettings::default()).unwrap();
    assert!(!finder.edges.is_empty());
}

#[test]
fn table_rows_and_columns() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let tables = page.find_tables(TableSettings::default()).unwrap();
    assert!(!tables.is_empty());
    let rows = tables[0].rows();
    let cols = tables[0].columns();
    assert!(!rows.is_empty());
    assert!(!cols.is_empty());
}

// ---------------------------------------------------------------------------
// 10. OBJECTS AND EDGES
// ---------------------------------------------------------------------------

#[test]
fn objects_showcase_has_all_types() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    assert!(!page.chars.is_empty());
    assert!(!page.lines.is_empty());
    assert!(!page.rects.is_empty());
    assert!(!page.curves.is_empty());
    assert!(!page.images.is_empty());
    assert!(!page.hyperlinks.is_empty());
}

#[test]
fn object_type_field_is_set() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    assert_eq!(page.chars[0].object_type, "char");
    assert_eq!(page.lines[0].object_type, "line");
    assert_eq!(page.rects[0].object_type, "rect");
    assert_eq!(page.curves[0].object_type, "curve");
    assert_eq!(page.images[0].object_type, "image");
    assert_eq!(page.annots[0].object_type, "annot");
    assert_eq!(page.hyperlinks[0].object_type, "hyperlink");
}

#[test]
fn object_page_number_matches() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    for ch in &page.chars {
        assert_eq!(ch.page_number, 1);
    }
    for line in &page.lines {
        assert_eq!(line.page_number, 1);
    }
    for img in &page.images {
        assert_eq!(img.page_number, 1);
    }
}

#[test]
fn edges_from_lines_and_rects() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let edges = page.edges();
    assert!(!edges.is_empty());

    let h_edges = page.horizontal_edges();
    let v_edges = page.vertical_edges();
    // Every edge is either horizontal or vertical
    assert_eq!(h_edges.len() + v_edges.len(), edges.len());
}

#[test]
fn rect_edges_and_curve_edges() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let rect_edges = page.rect_edges();
    let curve_edges = page.curve_edges();
    // We have rects and curves, so edges should exist from at least one
    let total = rect_edges.len() + curve_edges.len();
    assert!(total > 0 || page.rects.is_empty() && page.curves.is_empty());
}

#[test]
fn hyperlink_has_uri() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    assert!(!page.hyperlinks.is_empty());
    assert!(!page.hyperlinks[0].uri.is_empty());
}

#[test]
fn image_has_srcsize() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    assert!(!page.images.is_empty());
    let img = &page.images[0];
    assert!(img.srcsize.0 > 0);
    assert!(img.srcsize.1 > 0);
}

// ---------------------------------------------------------------------------
// 11. LAYOUT ANALYSIS
// ---------------------------------------------------------------------------

#[test]
fn textlinehorizontals_returns_lines() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.textlinehorizontals();
    assert!(!lines.is_empty());
    assert_eq!(lines[0].object_type, "textlinehorizontal");
    assert!(lines[0].text.is_some());
}

#[test]
fn textboxhorizontals_groups_lines() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let boxes = page.textboxhorizontals();
    assert!(!boxes.is_empty());
    assert_eq!(boxes[0].object_type, "textboxhorizontal");
}

#[test]
fn textlineverticals_on_rotated() {
    let pdf = PdfDocument::open(fixture("rotated_and_duplicates.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // May or may not have vertical text depending on content
    let _vlines = page.textlineverticals();
    // Just verify it doesn't panic
}

#[test]
fn layout_returns_all_types() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let layout = page.layout();
    assert_eq!(layout.page_number, 1);
    assert!(layout.bbox.is_valid());
    assert!(!layout.objects.is_empty());
}

#[test]
fn iter_layout_objects_flattens() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let flat: Vec<LayoutObject> = page.iter_layout_objects().collect();
    // Should have both textbox and textline objects
    assert!(!flat.is_empty());
}

#[test]
fn parse_objects_returns_map() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let map = page.parse_objects();
    assert!(
        map.contains_key("textlinehorizontal") || map.contains_key("textboxhorizontal")
    );
}

#[test]
fn derive_word_layout() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let word_layout = page.derive_word_layout();
    assert!(!word_layout.is_empty());
    assert_eq!(word_layout[0].object_type, "word");
}

#[test]
fn page_point2coord() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let (x, y) = page.point2coord((100.0, 200.0));
    assert_eq!(x, 100.0);
    assert_eq!(y, page.height - 200.0);
}

// ---------------------------------------------------------------------------
// 12. SERIALIZATION (JSON / CSV / DICT)
// ---------------------------------------------------------------------------

#[test]
fn page_to_dict_has_required_fields() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let dict = page.to_dict(None);
    assert!(dict.is_object());
    let obj = dict.as_object().unwrap();
    assert!(obj.contains_key("page_number"));
    assert!(obj.contains_key("width"));
    assert!(obj.contains_key("height"));
    assert!(obj.contains_key("bbox"));
    assert!(obj.contains_key("mediabox"));
    assert!(obj.contains_key("cropbox"));
    assert!(obj.contains_key("rotation"));
    assert!(obj.contains_key("is_original"));
    assert!(obj.contains_key("chars"));
}

#[test]
fn page_to_dict_with_object_type_filter() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let dict = page.to_dict(Some(&["char"]));
    let obj = dict.as_object().unwrap();
    assert!(obj.contains_key("chars"));
    // Should NOT contain lines, rects, etc. since we only asked for chars
    assert!(!obj.contains_key("lines"));
}

#[test]
fn page_to_json_roundtrip() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let json_str = page
        .to_json::<Vec<u8>>(None, None, None, None, None, Some(2))
        .unwrap()
        .unwrap();
    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn page_to_json_with_precision() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let json_str = page
        .to_json::<Vec<u8>>(None, None, None, None, Some(1), None)
        .unwrap()
        .unwrap();
    assert!(!json_str.is_empty());
}

#[test]
fn page_to_json_include_exclude_attrs() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // Include only specific attributes
    let json_str = page
        .to_json::<Vec<u8>>(None, None, Some(&["text", "x0", "top"]), None, None, Some(2))
        .unwrap()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    // chars should only have text, x0, top
    if let Some(chars) = parsed.get("chars").and_then(|c| c.as_array()) {
        if let Some(first) = chars.first() {
            assert!(first.get("text").is_some());
            assert!(first.get("fontname").is_none());
        }
    }
}

#[test]
fn page_to_csv_produces_valid_csv() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let csv = page
        .to_csv::<Vec<u8>>(None, None, None, None, None)
        .unwrap()
        .unwrap();
    assert!(!csv.is_empty());
    // Should have header row and data rows
    let line_count = csv.lines().count();
    assert!(line_count >= 2);
}

#[test]
fn page_to_csv_with_type_filter() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let csv = page
        .to_csv::<Vec<u8>>(None, Some(&["char"]), None, None, None)
        .unwrap()
        .unwrap();
    assert!(!csv.is_empty());
}

#[test]
fn document_to_json() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    let json_str = pdf
        .to_json::<Vec<u8>>(None, None, None, None, None, Some(2))
        .unwrap()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("pages").unwrap().as_array().unwrap().len() == 2);
    assert!(parsed.get("metadata").is_some());
}

#[test]
fn document_to_csv() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let csv = pdf
        .to_csv::<Vec<u8>>(None, None, None, None, None)
        .unwrap()
        .unwrap();
    assert!(!csv.is_empty());
}

#[test]
fn page_objects_map() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let objects = page.objects();
    assert!(objects.contains_key("char"));
    // annots is always included even if empty
    assert!(objects.contains_key("annot"));
}

// ---------------------------------------------------------------------------
// 13. DOCUMENT-LEVEL AGGREGATES
// ---------------------------------------------------------------------------

#[test]
fn document_chars_aggregates_all_pages() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    let total: usize = pdf.pages().iter().map(|p| p.chars.len()).sum();
    assert_eq!(pdf.chars().len(), total);
}

#[test]
fn document_lines_aggregates_all_pages() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let total: usize = pdf.pages().iter().map(|p| p.lines.len()).sum();
    assert_eq!(pdf.lines().len(), total);
}

#[test]
fn document_all_aggregates() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    // Just verify they don't panic and return consistent counts
    let _ = pdf.chars();
    let _ = pdf.lines();
    let _ = pdf.rects();
    let _ = pdf.curves();
    let _ = pdf.images();
    let _ = pdf.annots();
    let _ = pdf.hyperlinks();
    let _ = pdf.edges();
    let _ = pdf.rect_edges();
    let _ = pdf.curve_edges();
    let _ = pdf.horizontal_edges();
    let _ = pdf.vertical_edges();
}

#[test]
fn document_layout_aggregates() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let tl = pdf.textlinehorizontals();
    let tb = pdf.textboxhorizontals();
    assert!(!tl.is_empty());
    assert!(!tb.is_empty());
}

// ---------------------------------------------------------------------------
// 14. IMAGE RENDERING
// ---------------------------------------------------------------------------

#[test]
fn page_to_image_default_resolution() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let image = page.to_image(Some(72.0), None, None, false, false).unwrap();
    assert!(image.width() > 0);
    assert!(image.height() > 0);
}

#[test]
fn page_to_image_high_resolution() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lo = page.to_image(Some(72.0), None, None, false, false).unwrap();
    let hi = page.to_image(Some(144.0), None, None, false, false).unwrap();
    // 2x resolution should give ~2x pixels in each dimension
    assert!(hi.width() > lo.width());
    assert!(hi.height() > lo.height());
}

#[test]
fn page_to_image_by_width() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let image = page.to_image(None, Some(400.0), None, false, false).unwrap();
    assert!(image.width() > 0);
}

#[test]
fn page_to_image_by_height() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let image = page.to_image(None, None, Some(400.0), false, false).unwrap();
    assert!(image.height() > 0);
}

#[test]
fn page_to_image_multiple_size_params_errors() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let result = page.to_image(Some(72.0), Some(400.0), None, false, false);
    assert!(result.is_err());
}

#[test]
fn page_image_reset_and_copy() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut image = page.to_image(Some(72.0), None, None, false, false).unwrap();
    let copy = image.copy();
    assert_eq!(copy.width(), image.width());
    image.reset();
    assert_eq!(image.width(), copy.width());
}

#[test]
fn page_image_draw_operations() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut image = page.to_image(Some(72.0), None, None, false, false).unwrap();
    let red = RgbaColor::new(255, 0, 0, 255);
    let blue = RgbaColor::new(0, 0, 255, 128);

    // Test all drawing methods
    image.draw_hline(100.0, red, 1);
    image.draw_vline(100.0, red, 1);
    image.draw_hlines(&[200.0, 300.0], red, 1);
    image.draw_vlines(&[200.0, 300.0], red, 1);

    let bbox = BBox::new(50.0, 50.0, 200.0, 200.0);
    image.draw_rect(&bbox, Some(blue), Some(red), 1);

    let point = Point::new(100.0, 100.0);
    image.draw_circle(&point, 10, Some(red), None);

    let words = page.extract_words();
    if !words.is_empty() {
        image.outline_words(&words, red, 1);
    }
    image.outline_chars(&page.chars[..1.min(page.chars.len())], red, 1);
    image.outline_edges(&page.edges()[..1.min(page.edges().len())], red, 1);

    // Verify image still has valid dimensions
    assert!(image.width() > 0);
}

#[test]
fn page_image_debug_tablefinder() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let mut image = page.to_image(Some(72.0), None, None, false, false).unwrap();
    image.debug_tablefinder(None).unwrap();
    image.debug_tablefinder(Some(TableSettings::default())).unwrap();
}

#[test]
fn page_image_save_to_temp() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let image = page.to_image(Some(72.0), None, None, false, false).unwrap();
    let path = std::env::temp_dir().join("pdfsink_test_output.png");
    image
        .save(&path, Some(image::ImageFormat::Png), false, 256, 8)
        .unwrap();
    assert!(path.exists());
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// 15. SVG GENERATION
// ---------------------------------------------------------------------------

#[test]
fn debug_svg_is_valid() {
    let pdf = PdfDocument::open(fixture("objects_showcase.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let svg = page.to_debug_svg();
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert!(svg.contains("viewBox"));
    // Should contain text elements for chars
    assert!(svg.contains("<text"));
}

#[test]
fn debug_svg_empty_page() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    // Crop to empty region
    let empty = page.within_bbox(BBox::new(0.0, 0.0, 0.01, 0.01), false, false).unwrap();
    let svg = empty.to_debug_svg();
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
}

// ---------------------------------------------------------------------------
// 16. BBOX OPERATIONS
// ---------------------------------------------------------------------------

#[test]
fn bbox_basic_operations() {
    let bbox = BBox::new(10.0, 20.0, 100.0, 200.0);
    assert_eq!(bbox.width(), 90.0);
    assert_eq!(bbox.height(), 180.0);
    assert_eq!(bbox.area(), 90.0 * 180.0);
    assert!(bbox.is_valid());
    let center = bbox.center();
    assert_eq!(center.x, 55.0);
    assert_eq!(center.y, 110.0);
}

#[test]
fn bbox_overlap_and_contains() {
    let a = BBox::new(0.0, 0.0, 100.0, 100.0);
    let b = BBox::new(50.0, 50.0, 150.0, 150.0);
    let c = BBox::new(200.0, 200.0, 300.0, 300.0);
    let inner = BBox::new(10.0, 10.0, 90.0, 90.0);

    assert!(a.overlaps(b));
    assert!(!a.overlaps(c));
    assert!(a.contains_bbox(inner));
    assert!(!inner.contains_bbox(a));

    let overlap = a.overlap(b).unwrap();
    assert_eq!(overlap.x0, 50.0);
    assert_eq!(overlap.top, 50.0);
    assert!(a.overlap(c).is_none());
}

#[test]
fn bbox_translate() {
    let bbox = BBox::new(0.0, 0.0, 100.0, 100.0);
    let moved = bbox.translate(10.0, 20.0);
    assert_eq!(moved.x0, 10.0);
    assert_eq!(moved.top, 20.0);
    assert_eq!(moved.x1, 110.0);
    assert_eq!(moved.bottom, 120.0);
}

#[test]
fn bbox_as_tuple() {
    let bbox = BBox::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(bbox.as_tuple(), (1.0, 2.0, 3.0, 4.0));
}

#[test]
fn bbox_default_is_zero() {
    let bbox = BBox::default();
    assert_eq!(bbox.x0, 0.0);
    assert_eq!(bbox.area(), 0.0);
}

// ---------------------------------------------------------------------------
// 17. DIRECTION / ORIENTATION
// ---------------------------------------------------------------------------

#[test]
fn direction_parsing() {
    assert_eq!("ltr".parse::<Direction>().unwrap(), Direction::Ltr);
    assert_eq!("rtl".parse::<Direction>().unwrap(), Direction::Rtl);
    assert_eq!("ttb".parse::<Direction>().unwrap(), Direction::Ttb);
    assert_eq!("btt".parse::<Direction>().unwrap(), Direction::Btt);
    assert!("invalid".parse::<Direction>().is_err());
}

#[test]
fn direction_properties() {
    assert!(Direction::Ltr.is_horizontal());
    assert!(Direction::Rtl.is_horizontal());
    assert!(Direction::Ttb.is_vertical());
    assert!(Direction::Btt.is_vertical());
    assert_eq!(Direction::Ltr.as_str(), "ltr");
}

#[test]
fn table_strategy_parsing() {
    assert_eq!("lines".parse::<TableStrategy>().unwrap(), TableStrategy::Lines);
    assert_eq!("lines_strict".parse::<TableStrategy>().unwrap(), TableStrategy::LinesStrict);
    assert_eq!("text".parse::<TableStrategy>().unwrap(), TableStrategy::Text);
    assert!("invalid_strategy".parse::<TableStrategy>().is_err());
}

// ---------------------------------------------------------------------------
// 18. SERDE ROUNDTRIP
// ---------------------------------------------------------------------------

#[test]
fn page_serializes_to_json() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let json = serde_json::to_string(page).unwrap();
    assert!(!json.is_empty());
    // Deserialize back
    let page2: Page = serde_json::from_str(&json).unwrap();
    assert_eq!(page2.page_number, page.page_number);
    assert_eq!(page2.chars.len(), page.chars.len());
}

#[test]
fn document_serializes_to_json() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let json = serde_json::to_string(&pdf).unwrap();
    let pdf2: PdfDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(pdf2.len(), pdf.len());
    assert_eq!(pdf2.pages()[0].chars.len(), pdf.pages()[0].chars.len());
}

// ---------------------------------------------------------------------------
// 19. MULTIPAGE CONSISTENCY
// ---------------------------------------------------------------------------

#[test]
fn multipage_doctop_increases() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    let p1 = pdf.page(1).unwrap();
    let p2 = pdf.page(2).unwrap();
    assert!(p2.doctop_offset > p1.doctop_offset);
}

#[test]
fn multipage_chars_have_increasing_doctop() {
    let pdf = PdfDocument::open(fixture("multipage.pdf")).unwrap();
    let p1 = pdf.page(1).unwrap();
    let p2 = pdf.page(2).unwrap();
    if !p1.chars.is_empty() && !p2.chars.is_empty() {
        assert!(p2.chars[0].doctop > p1.chars[0].doctop);
    }
}

// ---------------------------------------------------------------------------
// 20. CROSS-FEATURE INTERACTIONS
// ---------------------------------------------------------------------------

#[test]
fn crop_then_extract_text() {
    let pdf = PdfDocument::open(fixture("crop_regions.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let cropped = page.crop(half, false, false).unwrap();
    let text = cropped.extract_text();
    // Cropped page should still have extractable text
    assert!(!text.is_empty());
}

#[test]
fn crop_then_find_tables() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let cropped = page.crop(page.bbox, false, false).unwrap();
    let tables = cropped.find_tables(TableSettings::default()).unwrap();
    // Full-page crop should still find the same tables
    assert!(!tables.is_empty());
}

#[test]
fn filter_then_extract_words() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let filtered = page.filter(|_| true);
    let words = filtered.extract_words();
    assert_eq!(words.len(), page.extract_words().len());
}

#[test]
fn dedupe_then_extract_text() {
    let pdf = PdfDocument::open(fixture("rotated_and_duplicates.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let deduped = page.dedupe_chars(&DedupeOptions::default());
    let text = deduped.extract_text();
    assert!(!text.is_empty());
}

#[test]
fn search_on_cropped_page() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let cropped = page.crop(page.bbox, false, false).unwrap();
    let matches = cropped.search("Hello").unwrap();
    assert!(!matches.is_empty());
}

#[test]
fn render_after_crop() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let cropped = page.crop(half, false, false).unwrap();
    let image = cropped.to_image(Some(72.0), None, None, false, false).unwrap();
    assert!(image.width() > 0);
}

#[test]
fn serialize_cropped_page() {
    let pdf = PdfDocument::open(fixture("simple_text.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let half = BBox::new(0.0, 0.0, page.width / 2.0, page.height);
    let cropped = page.crop(half, false, false).unwrap();
    let json = cropped
        .to_json::<Vec<u8>>(None, None, None, None, None, Some(2))
        .unwrap()
        .unwrap();
    assert!(!json.is_empty());
    assert!(json.contains("\"is_original\": false") || json.contains("\"is_original\":false"));
}

#[test]
fn layout_on_table_page() {
    let pdf = PdfDocument::open(fixture("table_lines.pdf")).unwrap();
    let page = pdf.page(1).unwrap();
    let lines = page.textlinehorizontals();
    let boxes = page.textboxhorizontals();
    // Table page should have text lines
    assert!(!lines.is_empty());
    assert!(!boxes.is_empty());
}

// ---------------------------------------------------------------------------
// 21. RESILIENT PARSING (malformed content)
// ---------------------------------------------------------------------------

#[test]
fn all_bench_pdfs_open_without_error() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return; // skip if bench PDFs not present
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let result = PdfDocument::open(&path);
            assert!(
                result.is_ok(),
                "Failed to open {}: {:?}",
                path.display(),
                result.unwrap_err()
            );
            let pdf = result.unwrap();
            assert!(pdf.len() > 0, "Empty document: {}", path.display());
            // Every page should have valid geometry even if content extraction failed
            for page in pdf.pages() {
                assert!(page.width > 0.0, "Zero width on {}", path.display());
                assert!(page.height > 0.0, "Zero height on {}", path.display());
                assert!(page.bbox.is_valid());
                assert!(page.mediabox.is_valid());
            }
        }
    }
}

#[test]
fn all_bench_pdfs_text_extraction_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for page in pdf.pages() {
                let _ = page.extract_text();
                let _ = page.extract_words();
                let _ = page.extract_text_lines(false, false);
            }
        }
    }
}

#[test]
fn all_bench_pdfs_search_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for page in pdf.pages() {
                let _ = page.search("the");
                let _ = page.search(".*");
            }
        }
    }
}

#[test]
fn all_bench_pdfs_table_extraction_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for page in pdf.pages() {
                let _ = page.find_tables(TableSettings::default());
                let _ = page.extract_tables(TableSettings::default());
            }
        }
    }
}

#[test]
fn all_bench_pdfs_layout_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for page in pdf.pages() {
                let _ = page.textlinehorizontals();
                let _ = page.textlineverticals();
                let _ = page.textboxhorizontals();
                let _ = page.textboxverticals();
                let _ = page.edges();
                let _ = page.layout();
            }
        }
    }
}

#[test]
fn all_bench_pdfs_serialization_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if let Some(page) = pdf.pages().first() {
                let _ = page.to_dict(None);
                let _ = page.to_json::<Vec<u8>>(None, None, None, None, None, None);
                let _ = page.to_csv::<Vec<u8>>(None, None, None, None, None);
                let _ = page.to_debug_svg();
            }
        }
    }
}

#[test]
fn all_bench_pdfs_render_no_panic() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/pdfs");
    if !bench_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&bench_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "pdf").unwrap_or(false) {
            let pdf = match PdfDocument::open(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if let Some(page) = pdf.pages().first() {
                let _ = page.to_image(Some(36.0), None, None, false, false);
            }
        }
    }
}
