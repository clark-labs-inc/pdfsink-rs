use super::*;

type PagePaths = (Vec<Line>, Vec<RectObject>, Vec<Curve>);

pub(super) fn collect(
    doc: &Document,
    page_id: ObjectId,
    resources: &Dictionary,
    geom: PageGeometry,
    page_number: usize,
) -> Result<PagePaths> {
    let content = doc.get_page_content(page_id)?;
    let mut walker = PagePathWalker {
        doc,
        collector: CollectorOutput::new(geom, page_number),
        active_forms: HashSet::new(),
        remaining_xobject_invocations: MAX_XOBJECT_INVOCATIONS_PER_PAGE,
    };
    walker.walk_stream(
        content,
        resources,
        Transform2D::<f64, Space, Space>::identity(),
        0,
    )?;
    let (_, lines, rects, curves) = walker.collector.finish();
    Ok((lines, rects, curves))
}

struct PagePathWalker<'a> {
    doc: &'a Document,
    collector: CollectorOutput,
    active_forms: HashSet<ObjectId>,
    remaining_xobject_invocations: usize,
}

impl PagePathWalker<'_> {
    fn walk_stream(
        &mut self,
        content: Vec<u8>,
        resources: &Dictionary,
        initial_ctm: Transform,
        form_depth: usize,
    ) -> Result<()> {
        let content = Content::decode(&content)?;
        let mut ctm = initial_ctm;
        let mut stack = Vec::new();
        let mut path = Path { ops: Vec::new() };
        let mut current_point = None;
        let mut subpath_start = None;

        for operation in content.operations {
            match operation.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => {
                    ctm = stack
                        .pop()
                        .unwrap_or_else(Transform2D::<f64, Space, Space>::identity);
                }
                "cm" => {
                    let matrix = transform_from_operands(&operation.operands)?;
                    ctm = ctm.pre_transform(&matrix);
                }
                "m" => {
                    if let Some(point) = point_from_operands(&operation.operands, 0) {
                        path.ops.push(PathOp::MoveTo(point.0, point.1));
                        current_point = Some(point);
                        subpath_start = Some(point);
                    }
                }
                "l" => {
                    if let Some(point) = point_from_operands(&operation.operands, 0) {
                        path.ops.push(PathOp::LineTo(point.0, point.1));
                        current_point = Some(point);
                    }
                }
                "c" => {
                    if let Some((control_1, control_2, point)) =
                        curve_from_operands(&operation.operands)
                    {
                        path.ops.push(PathOp::CurveTo(
                            control_1.0,
                            control_1.1,
                            control_2.0,
                            control_2.1,
                            point.0,
                            point.1,
                        ));
                        current_point = Some(point);
                    }
                }
                "v" => {
                    if let (Some(control_1), Some(control_2), Some(point)) = (
                        current_point,
                        point_from_operands(&operation.operands, 0),
                        point_from_operands(&operation.operands, 2),
                    ) {
                        path.ops.push(PathOp::CurveTo(
                            control_1.0,
                            control_1.1,
                            control_2.0,
                            control_2.1,
                            point.0,
                            point.1,
                        ));
                        current_point = Some(point);
                    }
                }
                "y" => {
                    if let (Some(control_1), Some(point)) = (
                        point_from_operands(&operation.operands, 0),
                        point_from_operands(&operation.operands, 2),
                    ) {
                        path.ops.push(PathOp::CurveTo(
                            control_1.0,
                            control_1.1,
                            point.0,
                            point.1,
                            point.0,
                            point.1,
                        ));
                        current_point = Some(point);
                    }
                }
                "h" => {
                    close_path(&mut path);
                    current_point = subpath_start;
                }
                "re" => {
                    if let Some((x, y, width, height)) =
                        rect_from_operands(&operation.operands)
                    {
                        path.ops.push(PathOp::Rect(x, y, width, height));
                        current_point = Some((x, y));
                        subpath_start = Some((x, y));
                    }
                }
                "S" => self.paint(&ctm, &mut path, true, false, false),
                "s" => self.paint(&ctm, &mut path, true, false, true),
                "F" | "f" | "f*" => {
                    self.paint(&ctm, &mut path, false, true, false)
                }
                "B" | "B*" => self.paint(&ctm, &mut path, true, true, false),
                "b" | "b*" => self.paint(&ctm, &mut path, true, true, true),
                "n" => path.ops.clear(),
                "Do" => {
                    if let Some(name) =
                        operation.operands.first().and_then(obj_to_name_string)
                    {
                        self.walk_xobject(resources, &name, ctm, form_depth)?;
                    }
                }
                _ => {}
            }

            if matches!(
                operation.operator.as_str(),
                "S" | "s" | "F" | "f" | "f*" | "B" | "B*" | "b" | "b*" | "n"
            ) {
                current_point = None;
                subpath_start = None;
            }
        }
        Ok(())
    }

    fn paint(
        &mut self,
        ctm: &Transform,
        path: &mut Path,
        stroke: bool,
        fill: bool,
        close: bool,
    ) {
        if close {
            close_path(path);
        }
        self.collector.push_path(ctm, path, stroke, fill);
        path.ops.clear();
    }

    fn walk_xobject(
        &mut self,
        resources: &Dictionary,
        name: &str,
        ctm: Transform,
        form_depth: usize,
    ) -> Result<()> {
        let Some(xobjects) = dict_get(resources, b"XObject") else {
            return Ok(());
        };
        let xobjects = object_to_dict(self.doc, xobjects)?;
        let Some(target) = dict_get(&xobjects, name.as_bytes()) else {
            return Ok(());
        };
        if self.remaining_xobject_invocations == 0 {
            return Err(Error::Message(format!(
                "page exceeds maximum of {MAX_XOBJECT_INVOCATIONS_PER_PAGE} XObject invocations"
            )));
        }
        self.remaining_xobject_invocations -= 1;

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
                return Err(Error::Message(
                    "cyclic Form XObject reference".to_string(),
                ));
            }
        }

        let form_resources = dict_get(&stream.dict, b"Resources")
            .map(|object| object_to_dict(self.doc, object))
            .transpose()?
            .unwrap_or_else(|| resources.clone());
        let form_matrix = dict_get(&stream.dict, b"Matrix")
            .map(transform_from_obj)
            .transpose()?
            .unwrap_or_else(Transform2D::<f64, Space, Space>::identity);
        let next_ctm = ctm.pre_transform(&form_matrix);
        let bytes = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let result =
            self.walk_stream(bytes, &form_resources, next_ctm, form_depth + 1);

        if let Some(object_id) = object_id {
            self.active_forms.remove(&object_id);
        }
        result
    }
}

fn point_from_operands(operands: &[Object], offset: usize) -> Option<(f64, f64)> {
    Some((
        obj_to_f64(operands.get(offset)?)?,
        obj_to_f64(operands.get(offset + 1)?)?,
    ))
}

fn curve_from_operands(
    operands: &[Object],
) -> Option<((f64, f64), (f64, f64), (f64, f64))> {
    Some((
        point_from_operands(operands, 0)?,
        point_from_operands(operands, 2)?,
        point_from_operands(operands, 4)?,
    ))
}

fn rect_from_operands(operands: &[Object]) -> Option<(f64, f64, f64, f64)> {
    Some((
        obj_to_f64(operands.first()?)?,
        obj_to_f64(operands.get(1)?)?,
        obj_to_f64(operands.get(2)?)?,
        obj_to_f64(operands.get(3)?)?,
    ))
}

fn close_path(path: &mut Path) {
    if !path.ops.is_empty()
        && !matches!(path.ops.last(), Some(PathOp::Close | PathOp::Rect(..)))
    {
        path.ops.push(PathOp::Close);
    }
}
