use super::*;
use std::io::Read;

mod free_text;

const MAX_ANNOTATION_APPEARANCES_PER_PAGE: usize = 1_024;
const MAX_ANNOTATION_APPEARANCE_BYTES_PER_PAGE: usize = 64 * 1024 * 1024;
const MAX_ANNOTATION_APPEARANCE_BYTES: usize = 8 * 1024 * 1024;

type AppearanceObjects = (Vec<Char>, Vec<Line>, Vec<RectObject>, Vec<Curve>);

struct AppearanceJob {
    content: Vec<u8>,
    resources: Object,
    clip: BBox,
}

pub(super) fn collect(
    doc: &Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
    geom: PageGeometry,
    page_number: usize,
    page_resources: &Dictionary,
) -> AppearanceObjects {
    let jobs = collect_jobs(doc, page_dict, geom, page_resources);
    let mut output = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    if jobs.is_empty() {
        return output;
    }

    let mut appearance_doc = doc.clone();
    for job in jobs {
        let content_id =
            appearance_doc.add_object(Stream::new(Dictionary::new(), job.content));
        if set_page_appearance(&mut appearance_doc, page_id, content_id, job.resources).is_err() {
            continue;
        }

        let mut collector = CollectorOutput::new(geom, page_number);
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pdf_extract::output_doc_page(
                &appearance_doc,
                &mut collector,
                page_number as u32,
            )
        }));
        if !matches!(parsed, Ok(Ok(()))) {
            continue;
        }

        let (mut chars, mut lines, mut rects, mut curves) = collector.finish();
        chars.retain(|object| {
            bbox_is_within(object.x0, object.top, object.x1, object.bottom, job.clip)
        });
        lines.retain(|object| {
            bbox_is_within(object.x0, object.top, object.x1, object.bottom, job.clip)
        });
        rects.retain(|object| {
            bbox_is_within(object.x0, object.top, object.x1, object.bottom, job.clip)
        });
        curves.retain(|object| {
            bbox_is_within(object.x0, object.top, object.x1, object.bottom, job.clip)
        });
        output.0.extend(chars);
        output.1.extend(lines);
        output.2.extend(rects);
        output.3.extend(curves);
    }
    output
}

fn collect_jobs(
    doc: &Document,
    page_dict: &Dictionary,
    geom: PageGeometry,
    page_resources: &Dictionary,
) -> Vec<AppearanceJob> {
    let Some(annots_obj) = dict_get(page_dict, b"Annots") else {
        return Vec::new();
    };
    let Ok(Object::Array(annots)) = deref_object(doc, annots_obj) else {
        return Vec::new();
    };

    let mut jobs = Vec::new();
    let mut total_bytes = 0usize;
    for annot_obj in annots
        .into_iter()
        .take(MAX_ANNOTATION_APPEARANCES_PER_PAGE)
    {
        let Ok(Object::Dictionary(annotation)) = deref_object(doc, &annot_obj) else {
            continue;
        };
        let flags = dict_get(&annotation, b"F")
            .and_then(obj_to_i64)
            .unwrap_or(0);
        if flags & (1 | 2 | 32) != 0 {
            continue;
        }
        let Some(rect) =
            dict_get(&annotation, b"Rect").and_then(|value| obj_to_box(value).ok())
        else {
            continue;
        };
        let subtype = dict_get(&annotation, b"Subtype").and_then(obj_to_name_string);
        let stream = if subtype.as_deref() == Some("Widget") {
            selected_widget_normal_appearance(doc, &annotation)
        } else if dict_get(&annotation, b"AP").is_some() {
            direct_normal_appearance(doc, &annotation)
        } else if subtype.as_deref() == Some("FreeText") {
            free_text::synthesize(doc, &annotation, rect, page_resources)
        } else {
            None
        };
        let Some(stream) = stream else {
            continue;
        };
        let remaining = MAX_ANNOTATION_APPEARANCE_BYTES_PER_PAGE.saturating_sub(total_bytes);
        let Some(job) =
            prepare_job(doc, &stream, rect, geom, page_resources, remaining)
        else {
            continue;
        };
        let Some(next_total) = total_bytes.checked_add(job.content.len()) else {
            break;
        };
        if next_total > MAX_ANNOTATION_APPEARANCE_BYTES_PER_PAGE {
            break;
        }
        total_bytes = next_total;
        jobs.push(job);
    }
    jobs
}

fn selected_widget_normal_appearance(doc: &Document, widget: &Dictionary) -> Option<Stream> {
    let appearance = object_to_dict(doc, dict_get(widget, b"AP")?).ok()?;
    let normal = deref_object(doc, dict_get(&appearance, b"N")?).ok()?;
    match normal {
        Object::Stream(stream) => is_form_appearance(&stream).then_some(stream),
        Object::Dictionary(states) => {
            let active_state = active_state_name(doc, widget)?;
            let selected = dict_get(&states, active_state.as_bytes())?;
            match deref_object(doc, selected).ok()? {
                Object::Stream(stream) if is_form_appearance(&stream) => Some(stream),
                _ => None,
            }
        }
        _ => None,
    }
}

fn direct_normal_appearance(doc: &Document, annotation: &Dictionary) -> Option<Stream> {
    let appearance = object_to_dict(doc, dict_get(annotation, b"AP")?).ok()?;
    match deref_object(doc, dict_get(&appearance, b"N")?).ok()? {
        Object::Stream(stream) if is_form_appearance(&stream) => Some(stream),
        _ => None,
    }
}

fn active_state_name(doc: &Document, widget: &Dictionary) -> Option<String> {
    if let Some(state) = dict_get(widget, b"AS").and_then(obj_to_name_string) {
        return Some(state);
    }

    let mut current = widget.clone();
    for _ in 0..MAX_PAGE_PARENT_DEPTH {
        if let Some(value) = dict_get(&current, b"V").and_then(obj_to_name_string) {
            return Some(value);
        }
        let parent = dict_get(&current, b"Parent").and_then(obj_to_reference)?;
        current = object_to_dict(doc, &Object::Reference(parent)).ok()?;
    }
    None
}

fn is_form_appearance(stream: &Stream) -> bool {
    dict_get(&stream.dict, b"Subtype")
        .and_then(obj_to_name_string)
        .as_deref()
        == Some("Form")
}

fn prepare_job(
    doc: &Document,
    stream: &Stream,
    rect: [f64; 4],
    geom: PageGeometry,
    page_resources: &Dictionary,
    remaining_bytes: usize,
) -> Option<AppearanceJob> {
    let bbox = obj_to_box(dict_get(&stream.dict, b"BBox")?).ok()?;
    let matrix = dict_get(&stream.dict, b"Matrix")
        .map(transform_from_obj)
        .transpose()
        .ok()?
        .unwrap_or_else(Transform2D::<f64, Space, Space>::identity);
    let transformed_bbox = transformed_box(bbox, &matrix)?;
    let rect_x0 = rect[0].min(rect[2]);
    let rect_y0 = rect[1].min(rect[3]);
    let rect_width = (rect[2] - rect[0]).abs();
    let rect_height = (rect[3] - rect[1]).abs();
    if rect_width == 0.0 || rect_height == 0.0 {
        return None;
    }

    let bbox_width = transformed_bbox[2] - transformed_bbox[0];
    let bbox_height = transformed_bbox[3] - transformed_bbox[1];
    if bbox_width <= 0.0 || bbox_height <= 0.0 {
        return None;
    }
    let scale_x = rect_width / bbox_width;
    let scale_y = rect_height / bbox_height;
    let translate_x = rect_x0 - transformed_bbox[0] * scale_x;
    let translate_y = rect_y0 - transformed_bbox[1] * scale_y;
    if ![
        scale_x,
        scale_y,
        translate_x,
        translate_y,
        matrix.m11,
        matrix.m12,
        matrix.m21,
        matrix.m22,
        matrix.m31,
        matrix.m32,
    ]
    .into_iter()
    .all(f64::is_finite)
    {
        return None;
    }

    let clip = format!(
        "{} {} {} {} re W n\n",
        bbox[0].min(bbox[2]),
        bbox[1].min(bbox[3]),
        (bbox[2] - bbox[0]).abs(),
        (bbox[3] - bbox[1]).abs()
    );
    let prefix = format!(
        "q\n{scale_x} 0 0 {scale_y} {translate_x} {translate_y} cm\n\
         {} {} {} {} {} {} cm\n{clip}",
        matrix.m11, matrix.m12, matrix.m21, matrix.m22, matrix.m31, matrix.m32
    );
    let overhead = prefix.len().checked_add(3)?;
    let content_limit = remaining_bytes
        .saturating_sub(overhead)
        .min(MAX_ANNOTATION_APPEARANCE_BYTES);
    let content = bounded_stream_content(stream, content_limit)?;
    let decoded = Content::decode(&content).ok()?;
    if decoded.operations.iter().any(|operation| operation.operator == "Do") {
        return None;
    }

    let resources = dict_get(&stream.dict, b"Resources")
        .cloned()
        .unwrap_or_else(|| Object::Dictionary(page_resources.clone()));
    object_to_dict(doc, &resources).ok()?;

    let mut page_content = Vec::with_capacity(prefix.len() + content.len() + 3);
    page_content.extend_from_slice(prefix.as_bytes());
    page_content.extend_from_slice(&content);
    page_content.extend_from_slice(b"\nQ\n");

    Some(AppearanceJob {
        content: page_content,
        resources,
        clip: annotation_bbox(rect, geom)?,
    })
}

fn annotation_bbox(rect: [f64; 4], geom: PageGeometry) -> Option<BBox> {
    let points = [
        geom.map_raw_point(rect[0], rect[1]),
        geom.map_raw_point(rect[2], rect[1]),
        geom.map_raw_point(rect[2], rect[3]),
        geom.map_raw_point(rect[0], rect[3]),
    ];
    bbox_from_points(&points)
}

fn bbox_is_within(x0: f64, top: f64, x1: f64, bottom: f64, clip: BBox) -> bool {
    const EPSILON: f64 = 1e-6;
    [x0, top, x1, bottom].into_iter().all(f64::is_finite)
        && x0 >= clip.x0 - EPSILON
        && top >= clip.top - EPSILON
        && x1 <= clip.x1 + EPSILON
        && bottom <= clip.bottom + EPSILON
}

fn bounded_stream_content(stream: &Stream, limit: usize) -> Option<Vec<u8>> {
    if limit == 0 || stream.content.len() > MAX_ANNOTATION_APPEARANCE_BYTES {
        return None;
    }
    let filters = if dict_get(&stream.dict, b"Filter").is_some() {
        stream.filters().ok()?
    } else {
        Vec::new()
    };
    match filters.as_slice() {
        [] => (stream.content.len() <= limit).then(|| stream.content.clone()),
        [b"FlateDecode"] if dict_get(&stream.dict, b"DecodeParms").is_none() => {
            let mut decoder = flate2::read::ZlibDecoder::new(stream.content.as_slice());
            let read_limit = u64::try_from(limit).ok()?.checked_add(1)?;
            let mut content = Vec::new();
            decoder
                .by_ref()
                .take(read_limit)
                .read_to_end(&mut content)
                .ok()?;
            (content.len() <= limit).then_some(content)
        }
        _ => None,
    }
}

fn transformed_box(raw: [f64; 4], matrix: &Transform) -> Option<[f64; 4]> {
    let x0 = raw[0].min(raw[2]);
    let y0 = raw[1].min(raw[3]);
    let x1 = raw[0].max(raw[2]);
    let y1 = raw[1].max(raw[3]);
    let points = [
        matrix.transform_point(Point2D::<f64, Space>::new(x0, y0)),
        matrix.transform_point(Point2D::<f64, Space>::new(x1, y0)),
        matrix.transform_point(Point2D::<f64, Space>::new(x1, y1)),
        matrix.transform_point(Point2D::<f64, Space>::new(x0, y1)),
    ];
    let min_x = points.iter().map(|point| point.x).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|point| point.y).fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|point| point.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = points
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);
    [min_x, min_y, max_x, max_y]
        .into_iter()
        .all(f64::is_finite)
        .then_some([min_x, min_y, max_x, max_y])
}

fn set_page_appearance(
    doc: &mut Document,
    page_id: ObjectId,
    content_id: ObjectId,
    resources: Object,
) -> Result<()> {
    let page = doc.get_object_mut(page_id)?;
    let dict = match page {
        Object::Dictionary(dict) => dict,
        Object::Stream(stream) => &mut stream.dict,
        other => {
            return Err(Error::Type(format!(
                "page object is not a dictionary: {other:?}"
            )));
        }
    };
    dict.set("Contents", Object::Reference(content_id));
    dict.set("Resources", resources);
    Ok(())
}
