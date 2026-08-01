# pdfsink-rs — Fast Pure-Rust PDF Text, Table & Layout Extraction

[![Crates.io](https://img.shields.io/crates/v/pdfsink-rs.svg)](https://crates.io/crates/pdfsink-rs)
[![Docs.rs](https://docs.rs/pdfsink-rs/badge.svg)](https://docs.rs/pdfsink-rs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)

**pdfsink-rs** is a fast, native, pure-Rust PDF extraction library and CLI — a drop-in conceptual replacement for Python's [pdfplumber](https://github.com/jsvine/pdfplumber) that runs **~10–50x faster** with zero Python runtime. Use it to parse PDFs, extract text, words, tables, layout, images, and metadata from Rust — with resilient handling of malformed documents.

**Keywords:** rust pdf library · rust pdf parser · pdf text extraction · pdf table extraction · pdfplumber alternative · rust pdf crate · extract text from pdf rust · extract tables from pdf rust · pdf layout analysis · pdf to json

Built and maintained by Stanislav Kirdey, [Clark Labs Inc.](https://github.com/clark-labs-inc)

## Why pdfsink-rs?

- **Fastest pure-Rust PDF extraction** — 10–50x faster than pdfplumber on real-world PDFs, with no Python interpreter, no native C bindings, no shell-outs.
- **Drop-in conceptual API** — mirrors pdfplumber's mental model (`chars`, `lines`, `rects`, `words`, `tables`, `extract_text`, `extract_table`, crop/within_bbox), so migrating a pipeline from Python to Rust is mostly renaming method calls.
- **Resilient** — malformed or damaged pages are recovered gracefully instead of crashing the whole document.
- **Batteries included** — text, word, line, table, layout, image, annotation, hyperlink, metadata, and tagged-structure extraction in a single crate.
- **Production-ready output formats** — JSON (with precision and filter control), CSV, and dictionary export for pipelines and LLM ingestion.
- **Ships a CLI** — `pdfsink-rs` binary for inspecting, debugging, and exporting PDFs from the shell without writing any Rust.

### Use cases

- Data pipelines extracting text and tables from government, financial, legal, and scientific PDFs
- LLM / RAG document ingestion requiring fast, structured PDF parsing in Rust
- PDF-to-JSON / PDF-to-CSV conversion at scale
- Layout-aware document understanding (textlines, textboxes, hierarchical layout trees)
- Rendering PDF pages to PNG / JPEG for previews or OCR pre-processing
- Migrating Python pdfplumber codebases to a faster native Rust stack

## Benchmarks: pdfsink-rs vs pdfplumber

Tested against pdfplumber 0.11.8 on real-world government PDFs.

### Speed

| PDF | Pages | Size | pdfsink-rs | pdfplumber | Speedup |
|-----|------:|-----:|-----------:|-----------:|--------:|
| US Budget FY2025 | 188 | 2.4 MB | 775 ms | 11.1 s | **14x** |
| NIST SP 800-53 | 492 | 6.1 MB | 4.07 s | 33.9 s | **8x** |
| IRS W-9 | 6 | 138 KB | 31 ms | 509 ms | **17x** |
| UN Charter | 54 | 3.0 MB | 130 ms | 1.95 s | **15x** |
| Census Table | 1 | 58 KB | 4.0 ms | 40 ms | **10x** |
| EPA Guide | 10 | — | 8 ms | 335 ms | **42x** |
| **Total (13 PDFs)** | | | **5.0 s** | **48.4 s** | **9.6x** |

Text extraction is **~34x faster**. Table extraction is **~253x faster**. Gracefully handles malformed pages that crash other parsers.

### Accuracy

| Metric | Result |
|--------|--------|
| Text similarity vs pdfplumber | **99.7%** |
| Word count match | **21/21 pages** |
| Character count match | **exact on all PDFs** |
| Page dimensions | **exact on all PDFs** |
| Line/rect object counts | **exact on matching PDFs** |
| Table detection (simple PDFs) | **1:1 match** |

## Features

- Open a PDF and access pages with full metadata
- **Resilient parsing** — malformed pages recovered gracefully (geometry preserved, content skipped)
- Inspect page objects (`chars`, `lines`, `rects`, `curves`, `images`, `annots`, `hyperlinks`)
- Crop / within-bbox / outside-bbox filtering
- Text extraction, word extraction, line extraction, regex search
- Table finding and table extraction (lines, lines_strict, text, explicit strategies)
- **Layout analysis** — textlines, textboxes, hierarchical layout tree
- **Serialization** — JSON (with precision/filtering), CSV, dictionary export
- **Image rendering** — rasterize pages to PNG/JPEG with drawing primitives
- **Document metadata** — mediabox, cropbox, trimbox, bleedbox, artbox
- **Structure tree** — tagged PDF structure element access
- **Document aggregates** — `chars()`, `lines()`, `rects()`, `edges()`, etc. across all pages
- CLI for inspection, debugging, and export

## Installation

Add the crate to your `Cargo.toml`:

```bash
cargo add pdfsink-rs
```

Or in `Cargo.toml`:

```toml
[dependencies]
pdfsink-rs = "0.2.14"
```

Requires Rust **1.97+**.

## Quick Start — Extract Text and Tables from a PDF in Rust

```rust
use pdfsink_rs::PdfDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pdf = PdfDocument::open("document.pdf")?;
    let page = pdf.page(1)?;

    // Extract text
    println!("{}", page.extract_text());

    // Extract words with positions
    for word in page.extract_words() {
        println!("{} @ ({}, {})", word.text, word.x0, word.top);
    }

    // Extract tables
    use pdfsink_rs::TableSettings;
    if let Some(table) = page.extract_table(TableSettings::default())? {
        for row in &table {
            println!("{:?}", row);
        }
    }

    // Layout analysis
    for tl in page.textlinehorizontals() {
        println!("line: {:?} @ ({}, {})", tl.text, tl.x0, tl.top);
    }

    // Serialize to JSON with precision control
    let json = page.to_json::<Vec<u8>>(None, None, None, None, Some(2), Some(2))?;
    println!("{}", json.unwrap_or_default());

    // Render page to PNG
    let image = page.to_image(Some(150.0), None, None, false, false)?;
    image.save("page.png", Some(image::ImageFormat::Png), false, 256, 8)?;

    Ok(())
}
```

## Browser / WASM Input (Upload Bytes)

Use `PdfDocument::from_bytes` when your PDF comes from an upload buffer instead of a filesystem path.

```rust
use pdfsink_rs::PdfDocument;

fn parse_uploaded_pdf(bytes: Vec<u8>) -> Result<String, pdfsink_rs::Error> {
    let pdf = PdfDocument::from_bytes(bytes)?;
    Ok(pdf.extract_text())
}
```

This is the expected entrypoint for in-browser workflows where you receive file bytes from JavaScript and pass them into Rust/WASM.

## CLI

```text
pdfsink-rs info <file.pdf>
pdfsink-rs text <file.pdf> [page]
pdfsink-rs words <file.pdf> [page]
pdfsink-rs search <file.pdf> [page] [pattern]
pdfsink-rs objects <file.pdf> [page]
pdfsink-rs json <file.pdf> [page]
pdfsink-rs csv <file.pdf> [page]
pdfsink-rs links <file.pdf> [page]
pdfsink-rs table <file.pdf> [page] [lines|lines_strict|text|explicit]
pdfsink-rs svg <file.pdf> [page] [output.svg]
pdfsink-rs render <file.pdf> [page] [output.png]
```

## Architecture

Built on `lopdf` for PDF parsing and `pdf-extract` for content stream processing. No Python runtime dependency.

| File | Purpose |
|------|---------|
| `src/lib.rs` | Public API (PdfDocument, Page methods) |
| `src/parse.rs` | PDF parsing, page-object extraction, metadata |
| `src/text.rs` | Text/word extraction, search, layout |
| `src/table.rs` | Table detection and extraction |
| `src/layout.rs` | Layout analysis (textlines, textboxes, layout tree) |
| `src/container_api.rs` | Serialization (JSON, CSV, dict export) |
| `src/display.rs` | Image rendering, drawing primitives |
| `src/geometry.rs` | Bbox operations, cropping, filtering |
| `src/clustering.rs` | Value clustering for layout analysis |

## Running Tests

```bash
cargo test
```

## Running Benchmarks

```bash
# Rust
cargo run --release --example bench_pdfsink

# pdfplumber (requires: pip install pdfplumber)
python bench/bench_pdfplumber.py

# Compare
python bench/compare.py
```

## FAQ

**Is pdfsink-rs a pure-Rust PDF library?**
Yes. Zero Python runtime, zero C bindings outside of standard Rust crates. It builds on `lopdf` (PDF object parser) and `pdf-extract` (content stream decoder) — both pure Rust.

**How does pdfsink-rs compare to pdfplumber?**
Same mental model, much faster. Benchmarked at ~10–50x faster across a 13-PDF real-world corpus, with 99.7% text similarity and exact character/word counts on matched pages.

**Can I extract tables from a PDF in Rust with pdfsink-rs?**
Yes. `page.extract_table(TableSettings::default())` and `find_tables` implement pdfplumber-style `lines`, `lines_strict`, `text`, and `explicit` strategies.

**Does it render PDFs to images?**
Yes. `page.to_image()` rasterizes pages to PNG or JPEG at any DPI.

**Does it handle malformed PDFs?**
Yes. Pages that fail content-stream parsing are reported but don't abort document-level extraction — geometry is preserved and bad content is skipped.

## Citation

Citation authorship: Stanislav Kirdey, Clark Labs Inc. See [`CITATION.cff`](CITATION.cff) for machine-readable citation metadata.

## License

MIT © Stanislav Kirdey, [Clark Labs Inc.](https://github.com/clark-labs-inc)

---

Built by **Stanislav Kirdey, Clark Labs Inc.** — the team behind [Clark Agent](https://www.clarkchat.com), AI-powered web automation and research. If pdfsink-rs saves you time, a ⭐ on [GitHub](https://github.com/clark-labs-inc/pdfsink-rs) helps others discover it.
