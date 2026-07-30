use super::*;
use lopdf::content::Operation;
use lopdf::Encoding;

const MAX_CONTENT_BYTES: usize = 16 * 1024;
const MAX_DEFAULT_APPEARANCE_BYTES: usize = 4 * 1024;
const MAX_LINES: usize = 256;

pub(super) fn synthesize(
    doc: &Document,
    annotation: &Dictionary,
    rect: [f64; 4],
    page_resources: &Dictionary,
) -> Option<Stream> {
    let contents_obj = dict_get(annotation, b"Contents")?;
    if encoded_string_len(contents_obj)? > MAX_CONTENT_BYTES {
        return None;
    }
    let contents = decode_pdf_string(contents_obj)?;
    if contents.is_empty() {
        return None;
    }

    let da = match dict_get(annotation, b"DA")? {
        Object::String(bytes, _) if bytes.len() <= MAX_DEFAULT_APPEARANCE_BYTES => bytes,
        _ => return None,
    };
    let (font_key, font_size) = parse_default_appearance(da)?;
    let resources = resources_with_standard_font(doc, page_resources, &font_key)?;

    let width = (rect[2] - rect[0]).abs();
    let height = (rect[3] - rect[1]).abs();
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || !font_size.is_finite()
        || !(0.1..=512.0).contains(&font_size)
    {
        return None;
    }

    let leading = font_size * 1.2;
    let padding = 2.0_f64.min(width / 4.0).min(height / 4.0);
    let baseline = (height - font_size - padding).max(padding);
    let encoding = Encoding::SimpleEncoding(b"WinAnsiEncoding");
    let mut operations = vec![
        Operation::new("BT", Vec::new()),
        Operation::new(
            "Tf",
            vec![Object::Name(font_key), Object::Real(font_size as f32)],
        ),
        Operation::new(
            "Td",
            vec![Object::Real(padding as f32), Object::Real(baseline as f32)],
        ),
    ];
    let mut line_count = 0usize;
    for line in contents.lines().take(MAX_LINES) {
        let encoded = encoding.string_to_bytes(line);
        if encoding.bytes_to_string(&encoded).ok().as_deref() != Some(line) {
            return None;
        }
        if line_count > 0 {
            operations.push(Operation::new(
                "Td",
                vec![Object::Integer(0), Object::Real(-(leading as f32))],
            ));
        }
        operations.push(Operation::new(
            "Tj",
            vec![Object::string_literal(encoded)],
        ));
        line_count += 1;
    }
    if line_count == 0 {
        return None;
    }
    operations.push(Operation::new("ET", Vec::new()));
    let content = Content { operations }.encode().ok()?;

    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set(
        "BBox",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(width as f32),
            Object::Real(height as f32),
        ]),
    );
    dict.set("Resources", Object::Dictionary(resources));
    Some(Stream::new(dict, content))
}

fn encoded_string_len(object: &Object) -> Option<usize> {
    match object {
        Object::String(bytes, _) => Some(bytes.len()),
        Object::Name(bytes) => Some(bytes.len()),
        _ => None,
    }
}

fn parse_default_appearance(da: &[u8]) -> Option<(Vec<u8>, f64)> {
    let content = Content::decode(da).ok()?;
    content.operations.into_iter().rev().find_map(|operation| {
        if operation.operator != "Tf" || operation.operands.len() != 2 {
            return None;
        }
        let Object::Name(font_key) = &operation.operands[0] else {
            return None;
        };
        let font_size = obj_to_f64(&operation.operands[1])?;
        Some((font_key.clone(), font_size))
    })
}

fn resources_with_standard_font(
    doc: &Document,
    page_resources: &Dictionary,
    font_key: &[u8],
) -> Option<Dictionary> {
    let mut resources = page_resources.clone();
    let mut fonts = dict_get(page_resources, b"Font")
        .map(|object| object_to_dict(doc, object))
        .transpose()
        .ok()?
        .unwrap_or_else(Dictionary::new);

    let base_font = dict_get(&fonts, font_key)
        .and_then(|font| object_to_dict(doc, font).ok())
        .and_then(|font| dict_get(&font, b"BaseFont").and_then(obj_to_name_string))
        .or_else(|| String::from_utf8(font_key.to_vec()).ok())?;
    if !is_standard_latin_font(&base_font) {
        return None;
    }

    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(base_font.into_bytes()));
    font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    fonts.set(font_key.to_vec(), Object::Dictionary(font));
    resources.set("Font", Object::Dictionary(fonts));
    Some(resources)
}

fn is_standard_latin_font(name: &str) -> bool {
    matches!(
        name,
        "Courier"
            | "Courier-Bold"
            | "Courier-Oblique"
            | "Courier-BoldOblique"
            | "Helvetica"
            | "Helvetica-Bold"
            | "Helvetica-Oblique"
            | "Helvetica-BoldOblique"
            | "Times-Roman"
            | "Times-Bold"
            | "Times-Italic"
            | "Times-BoldItalic"
    )
}
