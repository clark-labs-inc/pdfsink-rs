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
fn normal_widget_appearance_text_is_extracted_and_rendered() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /Rect [20 30 80 50] /AP << /N 6 0 R >> >>".to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Matrix [1 0 0 1 0 0] \
             /Resources << /Font << /F1 7 0 R >> >>",
            "BT /F1 10 Tf 2 5 Td (APTEXT) Tj ET",
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ]);
    let file = TempPdf::new("widget-normal-text", &pdf);

    let document = PdfDocument::open(file.path()).expect("open widget PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.extract_text().contains("APTEXT"));
    let image = page.to_image(Some(72.0), None, None, false, false).expect("render page");
    assert!(
        image.original.pixels().any(|pixel| pixel.0 != [255, 255, 255, 255]),
        "normal appearance must contribute rendered pixels"
    );
}

#[test]
fn widget_state_dictionary_uses_the_active_normal_appearance() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /Rect [20 30 80 50] /AS /Yes \
          /AP << /N << /Off 6 0 R /Yes 7 0 R >> >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "",
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "0 0 60 20 re f",
        ),
    ]);
    let file = TempPdf::new("widget-normal-state", &pdf);

    let document = PdfDocument::open(file.path()).expect("open stateful widget PDF");
    let page = document.page(1).expect("page 1");
    let rect = page.rects.first().expect("active appearance rectangle");

    assert_eq!((rect.x0, rect.top, rect.x1, rect.bottom), (20.0, 50.0, 80.0, 70.0));
}

#[test]
fn appearance_matrix_is_applied_before_widget_placement() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /Rect [20 30 80 50] /AP << /N 6 0 R >> >>".to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 30 10] /Matrix [2 0 0 2 5 7] \
             /Resources << >>",
            "0 0 30 10 re f",
        ),
    ]);
    let file = TempPdf::new("widget-normal-matrix", &pdf);

    let document = PdfDocument::open(file.path()).expect("open transformed widget PDF");
    let page = document.page(1).expect("page 1");
    let rect = page.rects.first().expect("transformed appearance rectangle");

    assert_eq!((rect.x0, rect.top, rect.x1, rect.bottom), (20.0, 50.0, 80.0, 70.0));
}

#[test]
fn absent_active_state_does_not_fall_back_to_a_checked_appearance() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /Rect [20 30 80 50] /AS /Off \
          /AP << /N << /Yes 6 0 R >> >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "0 0 60 20 re f",
        ),
    ]);
    let file = TempPdf::new("widget-missing-off-state", &pdf);

    let document = PdfDocument::open(file.path()).expect("open unchecked widget PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.rects.is_empty());
}

#[test]
fn missing_state_does_not_infer_the_only_non_off_appearance() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /Rect [20 30 80 50] \
          /AP << /N << /Yes 6 0 R >> >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "0 0 60 20 re f",
        ),
    ]);
    let file = TempPdf::new("widget-missing-state", &pdf);

    let document = PdfDocument::open(file.path()).expect("open stateless widget PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.rects.is_empty());
}

#[test]
fn hidden_widget_appearance_is_not_rendered() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Annots [5 0 R] >>".to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Widget /F 2 /Rect [20 30 80 50] \
          /AP << /N 6 0 R >> >>"
            .to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >>",
            "0 0 60 20 re f",
        ),
    ]);
    let file = TempPdf::new("widget-hidden", &pdf);

    let document = PdfDocument::open(file.path()).expect("open hidden widget PDF");
    let page = document.page(1).expect("page 1");

    assert!(page.rects.is_empty());
}
