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

fn build_pdf(objects: &[&str]) -> Vec<u8> {
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
fn negative_page_rotation_is_normalized_before_mapping_geometry() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Rotate -90 /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Text /Rect [10 20 30 40] >>",
    ]);
    let file = TempPdf::new("negative-rotation", &pdf);

    let document = PdfDocument::open(file.path()).expect("open rotated PDF");
    let page = document.page(1).expect("page 1");

    assert_eq!(page.rotation, 270);
    assert_eq!((page.width, page.height), (100.0, 200.0));
    assert_eq!(page.bbox.as_tuple(), (0.0, 0.0, 100.0, 200.0));

    let annotation = page.annots.first().expect("mapped annotation");
    assert_eq!(
        (annotation.x0, annotation.top, annotation.x1, annotation.bottom),
        (60.0, 170.0, 80.0, 190.0)
    );
}

#[test]
fn compound_rectangle_paths_contribute_transformed_geometry() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        "<< /Length 27 >>\nstream\n0 0 10 10 re 20 20 5 5 re f\nendstream",
    ]);
    let file = TempPdf::new("compound-rectangles", &pdf);

    let document = PdfDocument::open(file.path()).expect("open compound-path PDF");
    let page = document.page(1).expect("page 1");
    let curve = page.curves.first().expect("compound path geometry");

    assert_eq!((curve.x0, curve.top, curve.x1, curve.bottom), (0.0, 75.0, 25.0, 100.0));
    assert_eq!(curve.pts.len(), 8);
}
