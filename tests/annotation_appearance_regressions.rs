use pdfsink_rs::PdfDocument;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct TempPdf(PathBuf);

impl TempPdf {
    fn new(name: &str, bytes: &[u8]) -> Self {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pdfsink-rs-{name}-{}-{sequence}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write temporary PDF");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn stream(dictionary: &str, content: &str) -> String {
    format!(
        "<< {dictionary} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

fn build_pdf(objects: &[String]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[test]
fn direct_non_widget_normal_appearance_is_extracted_and_rendered() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /Rect [20 30 80 50] \
          /Contents (metadata only) /AP << /N 6 0 R >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] \
             /Resources << /Font << /F1 7 0 R >> >>",
            "0 0 60 20 re S BT /F1 10 Tf 2 5 Td (FITSAP) Tj ET",
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ]);
    let file = TempPdf::new("annotation-direct-normal", &pdf);

    let document = PdfDocument::open(file.path()).expect("open annotation PDF");
    let page = document.page(1).expect("page 1");

    assert!(
        page.extract_text().contains("FITSAP"),
        "extracted chars: {:?}",
        page.chars
    );
    assert_eq!(
        (page.rects[0].x0, page.rects[0].top, page.rects[0].x1, page.rects[0].bottom),
        (20.0, 50.0, 80.0, 70.0)
    );
    let image = page
        .to_image(Some(72.0), None, None, false, false)
        .expect("render page");
    assert!(
        image
            .original
            .pixels()
            .any(|pixel| pixel.0 != [255, 255, 255, 255]),
        "normal appearance must contribute rendered pixels"
    );
}

#[test]
fn non_widget_state_dictionary_is_not_inferred_or_replaced_by_synthesis() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /Rect [20 30 80 50] \
          /Contents (DO NOT INFER) /DA (/Helvetica 10 Tf) \
          /AP << /N << /One 6 0 R >> >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "0 0 60 20 re f",
        ),
    ]);
    let file = TempPdf::new("annotation-state-dictionary", &pdf);

    let document = PdfDocument::open(file.path()).expect("open stateful annotation PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.chars.is_empty());
    assert!(page.rects.is_empty());
}

#[test]
fn appearance_objects_outside_the_annotation_rectangle_fail_closed() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Square /Rect [40 40 60 60] \
          /AP << /N 6 0 R >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources << >>",
            "-100 -100 220 220 re f",
        ),
    ]);
    let file = TempPdf::new("annotation-escape", &pdf);

    let document = PdfDocument::open(file.path()).expect("open escaping annotation PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.rects.is_empty());
}

#[test]
fn free_text_without_appearance_gets_a_bounded_plain_text_appearance() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /F 4 /Rect [10 40 110 65] \
          /Contents (Bonjour) /DA (0 0 0 rg /Helvetica 12 Tf) >>"
            .to_string(),
    ]);
    let file = TempPdf::new("freetext-no-appearance", &pdf);

    let document = PdfDocument::open(file.path()).expect("open FreeText PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.extract_text().contains("Bonjour"));
    assert!(page.chars.iter().all(|character| {
        character.x0 >= 10.0
            && character.x1 <= 110.0
            && character.top >= 35.0
            && character.bottom <= 65.0
    }));
}

#[test]
fn free_text_synthesis_preserves_win_ansi_french_text() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /F 4 /Rect [10 40 150 65] \
          /Contents <FEFF00430061006600E90020006400E9006A00E0> \
          /DA (/Helvetica 12 Tf 0 0 0 rg) >>"
            .to_string(),
    ]);
    let file = TempPdf::new("freetext-french", &pdf);

    let document = PdfDocument::open(file.path()).expect("open French FreeText PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.extract_text().contains("Café déjà"));
}

#[test]
fn hidden_free_text_without_appearance_is_not_synthesized() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /F 2 /Rect [10 40 90 65] \
          /Contents (SECRET) /DA (/Helvetica 12 Tf) >>"
            .to_string(),
    ]);
    let file = TempPdf::new("freetext-hidden", &pdf);

    let document = PdfDocument::open(file.path()).expect("open hidden FreeText PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.chars.is_empty());
}

#[test]
fn oversized_default_appearance_is_not_parsed_or_synthesized() {
    let oversized_da = format!("({}/Helvetica 12 Tf)", " ".repeat(4_096));
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        format!(
            "<< /Type /Annot /Subtype /FreeText /Rect [10 40 90 65] \
             /Contents (TOO LARGE) /DA {oversized_da} >>"
        ),
    ]);
    let file = TempPdf::new("freetext-oversized-da", &pdf);

    let document = PdfDocument::open(file.path()).expect("open oversized DA PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.chars.is_empty());
}

#[test]
fn unsupported_free_text_font_is_not_inferred() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Annots [5 0 R] >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /FreeText /Rect [10 40 90 65] \
          /Contents (NO INFERENCE) /DA (/UnknownFont 12 Tf) >>"
            .to_string(),
    ]);
    let file = TempPdf::new("freetext-unknown-font", &pdf);

    let document = PdfDocument::open(file.path()).expect("open unknown-font FreeText PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.chars.is_empty());
}
