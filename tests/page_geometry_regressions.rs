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

#[test]
fn combined_and_close_path_paint_operators_emit_independent_geometry() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        "<< /Length 50 >>\nstream\n10 80 10 10 re B\n20 60 10 10 re f\n30 40 10 10 re s\nendstream",
    ]);
    let file = TempPdf::new("combined-paint-operators", &pdf);

    let document = PdfDocument::open(file.path()).expect("open painted paths PDF");
    let page = document.page(1).expect("page 1");

    assert_eq!(page.rects.len(), 3);
    assert_eq!(
        page.rects
            .iter()
            .map(|rect| (
                rect.x0,
                rect.top,
                rect.x1,
                rect.bottom,
                rect.stroke,
                rect.fill,
            ))
            .collect::<Vec<_>>(),
        vec![
            (10.0, 10.0, 20.0, 20.0, true, true),
            (20.0, 30.0, 30.0, 40.0, false, true),
            (30.0, 50.0, 40.0, 60.0, true, false),
        ]
    );
    assert!(page.curves.is_empty());

    let image = page
        .to_image(Some(72.0), None, None, false, false)
        .expect("render painted paths");
    assert_eq!(image.original.get_pixel(10, 10).0, [0, 0, 0, 255]);
    assert_eq!(image.original.get_pixel(15, 15).0, [235, 235, 235, 255]);
}

#[test]
fn combined_paint_geometry_in_nested_forms_keeps_its_transform_scope() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
         /Resources << /XObject << /Fm1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 43 >>\nstream\nq 2 0 0 2 10 10 cm /Fm1 Do Q\n60 70 5 5 re B\nendstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] \
         /Matrix [1 0 0 1 5 0] /Resources << /XObject << /Fm2 6 0 R >> >> \
         /Length 7 >>\nstream\n/Fm2 Do\nendstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] \
         /Resources << >> /Length 12 >>\nstream\n1 2 3 4 re B\nendstream",
    ]);
    let file = TempPdf::new("nested-form-combined-paint", &pdf);

    let document = PdfDocument::open(file.path()).expect("open nested Form PDF");
    let page = document.page(1).expect("page 1");

    assert_eq!(page.rects.len(), 2);
    assert_eq!(
        page.rects
            .iter()
            .map(|rect| (
                rect.x0,
                rect.top,
                rect.x1,
                rect.bottom,
                rect.stroke,
                rect.fill,
            ))
            .collect::<Vec<_>>(),
        vec![
            (22.0, 78.0, 28.0, 86.0, true, true),
            (60.0, 25.0, 65.0, 30.0, true, true),
        ]
    );
}

#[test]
fn independent_subpaths_render_without_connecting_them() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        "<< /Length 27 >>\nstream\n0 0 10 10 re 20 20 5 5 re S\nendstream",
    ]);
    let file = TempPdf::new("independent-stroked-subpaths", &pdf);

    let document = PdfDocument::open(file.path()).expect("open compound-path PDF");
    let page = document.page(1).expect("page 1");
    let image = page
        .to_image(Some(72.0), None, None, false, false)
        .expect("render compound path");

    assert_eq!(
        image.original.get_pixel(0, 95).0,
        [0, 0, 0, 255],
        "the first rectangle's closing edge must be rendered"
    );
    assert_eq!(
        image.original.get_pixel(10, 85).0,
        [255, 255, 255, 255],
        "independent rectangle subpaths must not be joined by a diagonal"
    );
}

#[test]
fn cubic_curve_bounds_and_pixels_include_the_curve_extremum() {
    let pdf = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        "<< /Length 29 >>\nstream\n10 50 m 10 90 90 90 90 50 c S\nendstream",
    ]);
    let file = TempPdf::new("cubic-curve-extremum", &pdf);

    let document = PdfDocument::open(file.path()).expect("open cubic-curve PDF");
    let page = document.page(1).expect("page 1");
    let curve = page.curves.first().expect("cubic curve geometry");

    assert_eq!((curve.x0, curve.x1, curve.bottom), (10.0, 90.0, 50.0));
    assert!((curve.top - 20.0).abs() < 1e-6);

    let image = page
        .to_image(Some(72.0), None, None, false, false)
        .expect("render cubic curve");
    assert_eq!(image.original.get_pixel(50, 20).0, [0, 0, 0, 255]);
}
