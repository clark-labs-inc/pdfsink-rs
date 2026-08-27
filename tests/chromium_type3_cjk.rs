use pdfsink_rs::PdfDocument;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pdfsink-rs-chromium-type3-cjk-{}-{sequence}.pdf",
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

/// Mirrors Skia/Chromium's Type3 fallback shape: a flipped FontMatrix,
/// single-byte `/Differences` names (`g<hex glyph id>`), `/ToUnicode`, and
/// vector glyph procedures containing `d1` plus filled paths.
fn chromium_type3_cjk_pdf() -> Vec<u8> {
    let cmap = "/CIDInit /ProcSet findresource begin\n\
12 dict begin\n\
begincmap\n\
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n\
/CMapType 2 def\n\
1 begincodespacerange\n\
<00> <FF>\n\
endcodespacerange\n\
1 beginbfrange\n\
<01> <02> [<4E2D> <6587>]\n\
endbfrange\n\
endcmap\n\
CMapName currentdict /CMap defineresource pop\n\
end\n\
end";

    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 220 120] \
          /Resources << /Font << /F3 5 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        stream(
            "",
            "BT /F3 48 Tf 1 0 0 1 20 28 Tm <0102> Tj \
             /Span << /ActualText <FEFF5347> >> BDC <0302> Tj EMC ET",
        ),
        "<< /Type /Font /Subtype /Type3 /Name /F3 \
          /FontBBox [0 -1000 1000 0] /FontMatrix [0.001 0 0 -0.001 0 0] \
          /CharProcs << /g0 6 0 R /g2A 7 0 R /g2B 8 0 R /g2C 9 0 R >> \
          /Encoding << /Differences [0 /g0 /g2A /g2B /g2C] >> \
          /FirstChar 0 /LastChar 3 /Widths [0 1000 1000 1000] \
          /ToUnicode 10 0 R /Resources << >> >>"
            .to_string(),
        stream("", "0 0 d0"),
        stream(
            "",
            "1000 0 0 -1000 100 -900 d1 \
             100 -100 m 900 -100 l 900 -900 l 100 -900 l h \
             250 -250 m 750 -250 l 750 -750 l 250 -750 l h f*",
        ),
        stream(
            "",
            "1000 0 0 -1000 100 -900 d1 \
             100 -100 m 900 -100 l 500 -900 l h \
             300 -300 m 700 -300 l 500 -700 l h f*",
        ),
        stream(
            "",
            "1000 0 0 -1000 100 -900 d1 \
             100 -150 m 900 -150 l 900 -350 l 100 -350 l h \
             400 -100 m 600 -100 l 600 -900 l 400 -900 l h f*",
        ),
        stream("", cmap),
    ])
}

#[test]
fn chromium_type3_cjk_is_unicode_extractable() {
    let file = TempPdf::new(&chromium_type3_cjk_pdf());
    let document = PdfDocument::open(file.path()).expect("open Chromium-style Type3 PDF");
    let page = document.page(1).expect("page 1");

    assert_eq!(page.extract_text(), "中文升");
    assert_eq!(
        page.chars.iter().map(|ch| ch.text.as_str()).collect::<Vec<_>>(),
        ["中", "文", "升"]
    );
    assert!(page.chars.iter().all(|ch| ch.fontname.starts_with("Type3:")));
}

#[test]
fn chromium_type3_cjk_preview_paints_embedded_glyph_procedures() {
    let file = TempPdf::new(&chromium_type3_cjk_pdf());
    let document = PdfDocument::open(file.path()).expect("open Chromium-style Type3 PDF");
    let page = document.page(1).expect("page 1");
    let image = page
        .to_image(Some(72.0), None, None, false, false)
        .expect("render Type3 page");

    let ink = image
        .original
        .pixels()
        .filter(|pixel| pixel.0 != [255, 255, 255, 255])
        .count();
    assert!(ink > 1_000, "embedded glyph outlines must be filled, got {ink} ink pixels");
}
