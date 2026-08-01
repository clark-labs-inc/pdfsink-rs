use crate::types::{Curve, PathCommand, Point};

const CUBIC_RENDER_SEGMENTS: usize = 32;
const ROOT_EPSILON: f64 = 1e-12;

pub(crate) fn cubic_extrema_points(p0: Point, p1: Point, p2: Point, p3: Point) -> Vec<Point> {
    let mut parameters = Vec::with_capacity(4);
    append_cubic_extrema_parameters(&mut parameters, p0.x, p1.x, p2.x, p3.x);
    append_cubic_extrema_parameters(&mut parameters, p0.y, p1.y, p2.y, p3.y);

    let mut points = Vec::with_capacity(parameters.len() + 2);
    points.push(p0);
    points.push(p3);
    points.extend(
        parameters
            .into_iter()
            .map(|parameter| cubic_point(p0, p1, p2, p3, parameter)),
    );
    points
}

pub(crate) fn curve_line_segments(curve: &Curve) -> Vec<(Point, Point)> {
    if curve.path.is_empty() {
        return curve
            .pts
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect();
    }

    let mut segments = Vec::new();
    let mut point_index = 0usize;
    let mut current = None;
    let mut subpath_start = None;

    for command in &curve.path {
        match command {
            PathCommand::MoveTo(point) => {
                point_index = point_index.saturating_add(1);
                current = Some(*point);
                subpath_start = Some(*point);
            }
            PathCommand::LineTo(point) => {
                point_index = point_index.saturating_add(1);
                if let Some(start) = current {
                    segments.push((start, *point));
                }
                current = Some(*point);
            }
            PathCommand::CurveTo { c1, c2, p } => {
                point_index = point_index.saturating_add(1);
                if let Some(start) = current {
                    append_cubic_segments(&mut segments, start, *c1, *c2, *p);
                }
                current = Some(*p);
            }
            PathCommand::Rect {
                x,
                y,
                width,
                height,
            } => {
                let fallback = [
                    Point::new(*x, *y),
                    Point::new(*x + *width, *y),
                    Point::new(*x + *width, *y + *height),
                    Point::new(*x, *y + *height),
                ];
                let corners = curve
                    .pts
                    .get(point_index..point_index.saturating_add(4))
                    .and_then(|points| <[Point; 4]>::try_from(points).ok())
                    .unwrap_or(fallback);
                point_index = point_index.saturating_add(4);
                segments.extend([
                    (corners[0], corners[1]),
                    (corners[1], corners[2]),
                    (corners[2], corners[3]),
                    (corners[3], corners[0]),
                ]);
                current = Some(corners[0]);
                subpath_start = Some(corners[0]);
            }
            PathCommand::Close => {
                if let (Some(start), Some(end)) = (subpath_start, current) {
                    if start != end {
                        segments.push((end, start));
                    }
                    current = Some(start);
                }
            }
        }
    }

    if segments.is_empty() {
        curve
            .pts
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect()
    } else {
        segments
    }
}

fn append_cubic_extrema_parameters(
    parameters: &mut Vec<f64>,
    p0: f64,
    p1: f64,
    p2: f64,
    p3: f64,
) {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;

    if a.abs() <= ROOT_EPSILON {
        if b.abs() > ROOT_EPSILON {
            push_unit_parameter(parameters, -c / b);
        }
        return;
    }

    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return;
    }
    let root = discriminant.sqrt();
    push_unit_parameter(parameters, (-b + root) / (2.0 * a));
    push_unit_parameter(parameters, (-b - root) / (2.0 * a));
}

fn push_unit_parameter(parameters: &mut Vec<f64>, parameter: f64) {
    if parameter > 0.0
        && parameter < 1.0
        && parameter.is_finite()
        && !parameters
            .iter()
            .any(|existing| (existing - parameter).abs() <= ROOT_EPSILON)
    {
        parameters.push(parameter);
    }
}

fn append_cubic_segments(
    segments: &mut Vec<(Point, Point)>,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
) {
    let mut previous = p0;
    for step in 1..=CUBIC_RENDER_SEGMENTS {
        let parameter = step as f64 / CUBIC_RENDER_SEGMENTS as f64;
        let point = cubic_point(p0, p1, p2, p3, parameter);
        segments.push((previous, point));
        previous = point;
    }
}

fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, parameter: f64) -> Point {
    let inverse = 1.0 - parameter;
    let p0_weight = inverse * inverse * inverse;
    let p1_weight = 3.0 * inverse * inverse * parameter;
    let p2_weight = 3.0 * inverse * parameter * parameter;
    let p3_weight = parameter * parameter * parameter;
    Point::new(
        p0_weight * p0.x + p1_weight * p1.x + p2_weight * p2.x + p3_weight * p3.x,
        p0_weight * p0.y + p1_weight * p1.y + p2_weight * p2.y + p3_weight * p3.y,
    )
}
