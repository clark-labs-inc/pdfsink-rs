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

fn build_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
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

fn stream(dictionary: &str, content: &str) -> Vec<u8> {
    format!(
        "<< {dictionary} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
    .into_bytes()
}

fn page_with_forms(form_objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                .iter()
                .chain(b"/Resources << /XObject << /Fm1 5 0 R >> >> /Contents 4 0 R >>")
                .copied()
                .collect()
        ),
        stream("", "q /Fm1 Do Q"),
    ];
    objects.extend(form_objects);
    build_pdf(&objects)
}

#[test]
fn cyclic_page_parent_chain_is_rejected() {
    let pdf = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Parent 4 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Pages /Kids [] /Count 0 /Parent 2 0 R >>".to_vec(),
    ]);
    let file = TempPdf::new("cyclic-parent", &pdf);

    let error = PdfDocument::open(file.path()).expect_err("cyclic parent chain must fail");
    assert!(error.to_string().contains("cyclic page Parent chain"));
}

#[test]
fn self_referencing_form_xobject_is_contained() {
    let pdf = page_with_forms(vec![stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 1 1] \
         /Resources << /XObject << /Fm1 5 0 R >> >>",
        "/Fm1 Do",
    )]);
    let file = TempPdf::new("self-referencing-form", &pdf);

    let document = PdfDocument::open(file.path()).expect("recursive image walk must fail safely");
    assert!(document.page(1).unwrap().images.is_empty());
}

#[test]
fn indirectly_recursive_form_xobjects_are_contained() {
    let pdf = page_with_forms(vec![
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 1 1] \
             /Resources << /XObject << /Fm2 6 0 R >> >>",
            "/Fm2 Do",
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 1 1] \
             /Resources << /XObject << /Fm1 5 0 R >> >>",
            "/Fm1 Do",
        ),
    ]);
    let file = TempPdf::new("indirect-form-cycle", &pdf);

    let document = PdfDocument::open(file.path()).expect("indirect cycle must fail safely");
    assert!(document.page(1).unwrap().images.is_empty());
}

#[test]
fn finite_form_xobject_is_accepted() {
    let pdf = page_with_forms(vec![stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 1 1]",
        "q Q",
    )]);
    let file = TempPdf::new("finite-form", &pdf);

    let document = PdfDocument::open(file.path()).expect("finite Form XObject must remain valid");
    assert_eq!(document.len(), 1);
}
