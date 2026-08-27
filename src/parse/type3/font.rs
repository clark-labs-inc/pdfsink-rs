use super::super::*;
use std::array;

#[derive(Clone)]
pub(super) struct Type3Font {
    pub(super) fontname: String,
    pub(super) font_matrix: Transform,
    widths: [f64; 256],
    pub(super) unicode: [String; 256],
    glyph_names: [Option<Vec<u8>>; 256],
    char_procs: Dictionary,
    pub(super) resources: Dictionary,
}

impl Type3Font {
    pub(super) fn read(
        doc: &Document,
        resources: &Dictionary,
        resource_name: &[u8],
    ) -> Result<Option<Self>> {
        let Some(fonts) = dict_get(resources, b"Font") else {
            return Ok(None);
        };
        let fonts = object_to_dict(doc, fonts)?;
        let Some(font) = dict_get(&fonts, resource_name) else {
            return Ok(None);
        };
        let font = object_to_dict(doc, font)?;
        if dict_get(&font, b"Subtype")
            .and_then(obj_to_name_string)
            .as_deref()
            != Some("Type3")
        {
            return Ok(None);
        }

        let font_matrix = dict_get(&font, b"FontMatrix")
            .map(transform_from_obj)
            .transpose()?
            .unwrap_or_else(|| {
                Transform2D::<f64, Space, Space>::row_major(
                    0.001, 0.0, 0.0, 0.001, 0.0, 0.0,
                )
            });
        let fontname = dict_get(&font, b"BaseFont")
            .or_else(|| dict_get(&font, b"Name"))
            .and_then(obj_to_name_string)
            .unwrap_or_else(|| "Type3".to_string());
        let char_procs = dict_get(&font, b"CharProcs")
            .map(|object| object_to_dict(doc, object))
            .transpose()?
            .unwrap_or_default();
        let resources = dict_get(&font, b"Resources")
            .map(|object| object_to_dict(doc, object))
            .transpose()?
            .unwrap_or_default();

        Ok(Some(Self {
            fontname: format!("Type3:{fontname}"),
            font_matrix,
            widths: type3_widths(doc, &font),
            unicode: type3_unicode(doc, &font),
            glyph_names: type3_glyph_names(doc, &font),
            char_procs,
            resources,
        }))
    }

    pub(super) fn advance(&self, code: u8) -> f64 {
        self.widths[code as usize] * self.font_matrix.m11
    }

    pub(super) fn glyph_stream(&self, doc: &Document, code: u8) -> Result<Option<Vec<u8>>> {
        let Some(name) = self.glyph_names[code as usize].as_deref() else {
            return Ok(None);
        };
        let Some(glyph) = dict_get(&self.char_procs, name) else {
            return Ok(None);
        };
        let Object::Stream(stream) = deref_object(doc, glyph)? else {
            return Ok(None);
        };
        Ok(Some(
            stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone()),
        ))
    }
}

fn type3_widths(doc: &Document, font: &Dictionary) -> [f64; 256] {
    let mut result = [0.0; 256];
    let first = dict_get(font, b"FirstChar")
        .and_then(obj_to_i64)
        .unwrap_or(0)
        .clamp(0, 255) as usize;
    let widths = dict_get(font, b"Widths")
        .and_then(|object| deref_object(doc, object).ok())
        .and_then(|object| match object {
            Object::Array(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default();
    for (offset, width) in widths.iter().filter_map(obj_to_f64).enumerate() {
        if let Some(slot) = result.get_mut(first + offset) {
            *slot = width;
        }
    }
    result
}

fn type3_glyph_names(doc: &Document, font: &Dictionary) -> [Option<Vec<u8>>; 256] {
    let mut result = array::from_fn(|_| None);
    let differences = dict_get(font, b"Encoding")
        .and_then(|object| object_to_dict(doc, object).ok())
        .and_then(|encoding| dict_get(&encoding, b"Differences").cloned())
        .and_then(|object| deref_object(doc, &object).ok())
        .and_then(|object| match object {
            Object::Array(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default();
    let mut code = 0usize;
    for item in differences {
        match item {
            Object::Integer(value) if (0..=255).contains(&value) => code = value as usize,
            Object::Name(name) => {
                if code < result.len() {
                    result[code] = Some(name);
                    code += 1;
                }
            }
            _ => {}
        }
    }
    result
}

fn type3_unicode(doc: &Document, font: &Dictionary) -> [String; 256] {
    let mut unicode_font = font.clone();
    unicode_font.remove(b"Encoding");
    let unicode_encoding = unicode_font.get_font_encoding(doc).ok();
    let fallback_encoding = font.get_font_encoding(doc).ok();
    array::from_fn(|code| {
        let bytes = [code as u8];
        unicode_encoding
            .as_ref()
            .and_then(|encoding| encoding.bytes_to_string(&bytes).ok())
            .filter(|text| !text.is_empty() && !text.contains('\u{fffd}'))
            .or_else(|| {
                fallback_encoding
                    .as_ref()
                    .and_then(|encoding| encoding.bytes_to_string(&bytes).ok())
            })
            .unwrap_or_default()
    })
}
