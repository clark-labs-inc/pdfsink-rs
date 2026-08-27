use super::*;

mod font;

use font::Type3Font;

const MAX_TYPE3_GLYPH_INVOCATIONS_PER_PAGE: usize = 20_000;

pub(super) struct Type3Content {
    pub(super) chars: Vec<Char>,
    pub(super) lines: Vec<Line>,
    pub(super) rects: Vec<RectObject>,
    pub(super) curves: Vec<Curve>,
}

#[derive(Clone)]
struct TextState {
    font: Option<Type3Font>,
    font_size: f64,
    character_spacing: f64,
    word_spacing: f64,
    horizontal_scaling: f64,
    leading: f64,
    rise: f64,
    text_matrix: Transform,
    line_matrix: Transform,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            font_size: 0.0,
            character_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 1.0,
            leading: 0.0,
            rise: 0.0,
            text_matrix: Transform2D::identity(),
            line_matrix: Transform2D::identity(),
        }
    }
}

#[derive(Clone, Default)]
struct MarkedActualText {
    text: String,
    emitted: bool,
}

#[derive(Clone)]
struct GraphicsState {
    ctm: Transform,
    text: TextState,
    actual_text: Vec<Option<MarkedActualText>>,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            ctm: Transform2D::identity(),
            text: TextState::default(),
            actual_text: Vec::new(),
        }
    }
}

struct Type3Walker<'a> {
    doc: &'a Document,
    geom: PageGeometry,
    page_number: usize,
    chars: CollectorOutput,
    lines: Vec<Line>,
    rects: Vec<RectObject>,
    curves: Vec<Curve>,
    active_forms: HashSet<ObjectId>,
    remaining_glyph_invocations: usize,
}

pub(super) fn collect(
    doc: &Document,
    page_id: ObjectId,
    resources: &Dictionary,
    geom: PageGeometry,
    page_number: usize,
) -> Result<Type3Content> {
    let content = doc.get_page_content(page_id)?;
    let mut walker = Type3Walker {
        doc,
        geom,
        page_number,
        chars: CollectorOutput::new(geom, page_number),
        lines: Vec::new(),
        rects: Vec::new(),
        curves: Vec::new(),
        active_forms: HashSet::new(),
        remaining_glyph_invocations: MAX_TYPE3_GLYPH_INVOCATIONS_PER_PAGE,
    };
    walker.walk_stream(content, resources, GraphicsState::default(), 0)?;
    let (chars, _, _, _) = walker.chars.finish();
    Ok(Type3Content {
        chars,
        lines: walker.lines,
        rects: walker.rects,
        curves: walker.curves,
    })
}

pub(super) fn same_glyph_slot(candidate: &Char, replacement: &Char) -> bool {
    let tolerance = replacement.size.abs().mul_add(0.08, 0.5);
    (candidate.x0 - replacement.x0).abs() <= tolerance
        && (candidate.bottom - replacement.bottom).abs() <= tolerance
}

impl Type3Walker<'_> {
    fn walk_stream(
        &mut self,
        content: Vec<u8>,
        resources: &Dictionary,
        mut state: GraphicsState,
        form_depth: usize,
    ) -> Result<()> {
        let content = Content::decode(&content)?;
        let mut stack = Vec::new();

        for operation in content.operations {
            match operation.operator.as_str() {
                "q" => stack.push(state.clone()),
                "Q" => state = stack.pop().unwrap_or_default(),
                "cm" => {
                    let matrix = transform_from_operands(&operation.operands)?;
                    state.ctm = state.ctm.pre_transform(&matrix);
                }
                "BT" | "ET" => {
                    state.text.text_matrix = Transform2D::identity();
                    state.text.line_matrix = Transform2D::identity();
                }
                "Tf" => self.set_font(&mut state, resources, &operation.operands)?,
                "Tc" => set_number(&mut state.text.character_spacing, &operation.operands),
                "Tw" => set_number(&mut state.text.word_spacing, &operation.operands),
                "Tz" => {
                    if let Some(value) = operation.operands.first().and_then(obj_to_f64) {
                        state.text.horizontal_scaling = value / 100.0;
                    }
                }
                "TL" => set_number(&mut state.text.leading, &operation.operands),
                "Ts" => set_number(&mut state.text.rise, &operation.operands),
                "Tm" => {
                    let matrix = transform_from_operands(&operation.operands)?;
                    state.text.text_matrix = matrix;
                    state.text.line_matrix = matrix;
                }
                "Td" => move_text_line(&mut state.text, &operation.operands, false),
                "TD" => move_text_line(&mut state.text, &operation.operands, true),
                "T*" => next_text_line(&mut state.text),
                "Tj" => {
                    if let Some(bytes) = operation.operands.first().and_then(string_bytes) {
                        self.show_text(&mut state, bytes)?;
                    }
                }
                "TJ" => self.show_text_array(&mut state, &operation.operands)?,
                "'" => {
                    next_text_line(&mut state.text);
                    if let Some(bytes) = operation.operands.first().and_then(string_bytes) {
                        self.show_text(&mut state, bytes)?;
                    }
                }
                "\"" => {
                    if operation.operands.len() >= 3 {
                        state.text.word_spacing = obj_to_f64(&operation.operands[0]).unwrap_or(0.0);
                        state.text.character_spacing = obj_to_f64(&operation.operands[1]).unwrap_or(0.0);
                        next_text_line(&mut state.text);
                        if let Some(bytes) = string_bytes(&operation.operands[2]) {
                            self.show_text(&mut state, bytes)?;
                        }
                    }
                }
                "BDC" | "BMC" => {
                    let actual = operation
                        .operands
                        .get(1)
                        .and_then(|object| actual_text(self.doc, resources, object));
                    state.actual_text.push(actual.map(|text| MarkedActualText {
                        text,
                        emitted: false,
                    }));
                }
                "EMC" => {
                    state.actual_text.pop();
                }
                "Do" => {
                    if let Some(name) = operation.operands.first().and_then(obj_to_name_string) {
                        self.walk_form(resources, &name, &state, form_depth)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn set_font(
        &self,
        state: &mut GraphicsState,
        resources: &Dictionary,
        operands: &[Object],
    ) -> Result<()> {
        if operands.len() < 2 {
            return Ok(());
        }
        let Some(name) = operands.first().and_then(|object| match object {
            Object::Name(name) => Some(name.as_slice()),
            _ => None,
        }) else {
            return Ok(());
        };
        state.text.font = Type3Font::read(self.doc, resources, name)?;
        state.text.font_size = obj_to_f64(&operands[1]).unwrap_or(0.0);
        Ok(())
    }

    fn show_text_array(&mut self, state: &mut GraphicsState, operands: &[Object]) -> Result<()> {
        let Some(Object::Array(items)) = operands.first() else {
            return Ok(());
        };
        for item in items {
            if let Some(bytes) = string_bytes(item) {
                self.show_text(state, bytes)?;
            } else if let Some(adjustment) = obj_to_f64(item) {
                let tx = -adjustment / 1000.0
                    * state.text.font_size
                    * state.text.horizontal_scaling;
                translate_text_matrix(&mut state.text, tx, 0.0);
            }
        }
        Ok(())
    }

    fn show_text(&mut self, state: &mut GraphicsState, bytes: &[u8]) -> Result<()> {
        let Some(font) = state.text.font.clone() else {
            return Ok(());
        };
        for (index, code) in bytes.iter().copied().enumerate() {
            self.show_glyph(state, &font, code, index == 0)?;
        }
        Ok(())
    }

    fn show_glyph(
        &mut self,
        state: &mut GraphicsState,
        font: &Type3Font,
        code: u8,
        first_in_show: bool,
    ) -> Result<()> {
        if self.remaining_glyph_invocations == 0 {
            return Err(Error::Message(format!(
                "page exceeds maximum of {MAX_TYPE3_GLYPH_INVOCATIONS_PER_PAGE} Type3 glyph invocations"
            )));
        }
        self.remaining_glyph_invocations -= 1;

        let text = marked_actual_text(&mut state.actual_text, first_in_show)
            .unwrap_or_else(|| font.unicode[code as usize].clone());
        let advance = font.advance(code);
        let extraction_matrix = Transform2D::row_major(
            state.text.horizontal_scaling,
            0.0,
            0.0,
            1.0,
            0.0,
            state.text.rise,
        )
        .post_transform(&state.text.text_matrix.post_transform(&state.ctm));
        self.chars
            .push_char(&extraction_matrix, advance, state.text.font_size, &text);
        if let Some(ch) = self.chars.chars.last_mut() {
            ch.fontname.clone_from(&font.fontname);
        }

        if let Some(content) = font.glyph_stream(self.doc, code)? {
            let glyph_text_matrix = Transform2D::row_major(
                state.text.font_size * state.text.horizontal_scaling,
                0.0,
                0.0,
                state.text.font_size,
                0.0,
                state.text.rise,
            )
            .post_transform(&state.text.text_matrix.post_transform(&state.ctm));
            let glyph_ctm = font.font_matrix.post_transform(&glyph_text_matrix);
            let (lines, rects, curves) = page_paths::collect_stream(
                self.doc,
                content,
                &font.resources,
                self.geom,
                self.page_number,
                glyph_ctm,
            )?;
            self.lines.extend(lines);
            self.rects.extend(rects);
            self.curves.extend(curves);
        }

        let word_spacing = if code == b' ' {
            state.text.word_spacing
        } else {
            0.0
        };
        let tx = (advance * state.text.font_size
            + state.text.character_spacing
            + word_spacing)
            * state.text.horizontal_scaling;
        translate_text_matrix(&mut state.text, tx, 0.0);
        Ok(())
    }

    fn walk_form(
        &mut self,
        resources: &Dictionary,
        name: &str,
        state: &GraphicsState,
        form_depth: usize,
    ) -> Result<()> {
        let Some(xobjects) = dict_get(resources, b"XObject") else {
            return Ok(());
        };
        let xobjects = object_to_dict(self.doc, xobjects)?;
        let Some(target) = dict_get(&xobjects, name.as_bytes()) else {
            return Ok(());
        };
        let object_id = obj_to_reference(target);
        let Object::Stream(stream) = deref_object(self.doc, target)? else {
            return Ok(());
        };
        if dict_get(&stream.dict, b"Subtype")
            .and_then(obj_to_name_string)
            .as_deref()
            != Some("Form")
        {
            return Ok(());
        }
        if form_depth >= MAX_FORM_XOBJECT_DEPTH {
            return Err(Error::Message(format!(
                "Form XObject nesting exceeds maximum depth of {MAX_FORM_XOBJECT_DEPTH}"
            )));
        }
        if let Some(object_id) = object_id {
            if !self.active_forms.insert(object_id) {
                return Err(Error::Message("cyclic Form XObject reference".to_string()));
            }
        }

        let form_resources = dict_get(&stream.dict, b"Resources")
            .map(|object| object_to_dict(self.doc, object))
            .transpose()?
            .unwrap_or_else(|| resources.clone());
        let form_matrix = dict_get(&stream.dict, b"Matrix")
            .map(transform_from_obj)
            .transpose()?
            .unwrap_or_else(Transform2D::identity);
        let mut form_state = state.clone();
        form_state.ctm = form_state.ctm.pre_transform(&form_matrix);
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let result = self.walk_stream(content, &form_resources, form_state, form_depth + 1);
        if let Some(object_id) = object_id {
            self.active_forms.remove(&object_id);
        }
        result
    }
}

fn string_bytes(object: &Object) -> Option<&[u8]> {
    match object {
        Object::String(bytes, _) => Some(bytes),
        _ => None,
    }
}

fn set_number(target: &mut f64, operands: &[Object]) {
    if let Some(value) = operands.first().and_then(obj_to_f64) {
        *target = value;
    }
}

fn move_text_line(text: &mut TextState, operands: &[Object], set_leading: bool) {
    if operands.len() < 2 {
        return;
    }
    let tx = obj_to_f64(&operands[0]).unwrap_or(0.0);
    let ty = obj_to_f64(&operands[1]).unwrap_or(0.0);
    if set_leading {
        text.leading = -ty;
    }
    text.line_matrix = text
        .line_matrix
        .pre_transform(&Transform2D::create_translation(tx, ty));
    text.text_matrix = text.line_matrix;
}

fn next_text_line(text: &mut TextState) {
    text.line_matrix = text
        .line_matrix
        .pre_transform(&Transform2D::create_translation(0.0, -text.leading));
    text.text_matrix = text.line_matrix;
}

fn translate_text_matrix(text: &mut TextState, tx: f64, ty: f64) {
    text.text_matrix = text
        .text_matrix
        .pre_transform(&Transform2D::create_translation(tx, ty));
}

fn actual_text(doc: &Document, resources: &Dictionary, object: &Object) -> Option<String> {
    let properties = match object {
        Object::Dictionary(dictionary) => Some(dictionary.clone()),
        Object::Reference(_) => object_to_dict(doc, object).ok(),
        Object::Name(name) => dict_get(resources, b"Properties")
            .and_then(|object| object_to_dict(doc, object).ok())
            .and_then(|properties| dict_get(&properties, name).cloned())
            .and_then(|object| object_to_dict(doc, &object).ok()),
        _ => None,
    }?;
    dict_get(&properties, b"ActualText").and_then(decode_pdf_string)
}

fn marked_actual_text(
    stack: &mut [Option<MarkedActualText>],
    first_in_show: bool,
) -> Option<String> {
    let actual = stack.iter_mut().rev().find_map(Option::as_mut)?;
    if actual.emitted || !first_in_show {
        return Some(String::new());
    }
    actual.emitted = true;
    Some(actual.text.clone())
}
