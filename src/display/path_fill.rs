use crate::types::{BBox, Curve, PathCommand, Point};
use image::{Rgba, RgbaImage};

const CUBIC_STEPS: usize = 16;

pub(super) fn fill_curve(
    image: &mut RgbaImage,
    curve: &Curve,
    viewport: BBox,
    scale: f64,
    color: Rgba<u8>,
) {
    let contours = projected_contours(curve, viewport, scale);
    if contours.is_empty() {
        return;
    }

    let min_y = contours
        .iter()
        .flatten()
        .map(|point| point.y)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as u32;
    let max_y = contours
        .iter()
        .flatten()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(image.height() as f64) as u32;

    for y in min_y..max_y {
        let scan_y = y as f64 + 0.5;
        let mut intersections = Vec::new();
        for contour in &contours {
            for edge in contour.windows(2) {
                let (start, end) = (edge[0], edge[1]);
                if (start.y <= scan_y && scan_y < end.y)
                    || (end.y <= scan_y && scan_y < start.y)
                {
                    let t = (scan_y - start.y) / (end.y - start.y);
                    intersections.push(start.x + t * (end.x - start.x));
                }
            }
        }
        intersections.sort_by(f64::total_cmp);
        for pair in intersections.chunks_exact(2) {
            let start = pair[0].ceil().max(0.0) as u32;
            let end = pair[1].ceil().min(image.width() as f64) as u32;
            for x in start..end {
                image.put_pixel(x, y, color);
            }
        }
    }
}

fn projected_contours(curve: &Curve, viewport: BBox, scale: f64) -> Vec<Vec<Point>> {
    let mut contours = Vec::new();
    let mut current = Vec::new();
    let mut cursor = None;

    for command in &curve.path {
        match *command {
            PathCommand::MoveTo(point) => {
                finish_contour(&mut contours, &mut current);
                let point = project(point, viewport, scale);
                current.push(point);
                cursor = Some(point);
            }
            PathCommand::LineTo(point) => {
                let point = project(point, viewport, scale);
                current.push(point);
                cursor = Some(point);
            }
            PathCommand::CurveTo { c1, c2, p } => {
                let Some(start) = cursor else {
                    continue;
                };
                let c1 = project(c1, viewport, scale);
                let c2 = project(c2, viewport, scale);
                let end = project(p, viewport, scale);
                for step in 1..=CUBIC_STEPS {
                    current.push(cubic_point(
                        start,
                        c1,
                        c2,
                        end,
                        step as f64 / CUBIC_STEPS as f64,
                    ));
                }
                cursor = Some(end);
            }
            PathCommand::Rect {
                x,
                y,
                width,
                height,
            } => {
                finish_contour(&mut contours, &mut current);
                let corners = [
                    Point::new(x, y),
                    Point::new(x + width, y),
                    Point::new(x + width, y + height),
                    Point::new(x, y + height),
                    Point::new(x, y),
                ];
                contours.push(
                    corners
                        .into_iter()
                        .map(|point| project(point, viewport, scale))
                        .collect(),
                );
                cursor = None;
            }
            PathCommand::Close => {
                close_contour(&mut current);
                finish_contour(&mut contours, &mut current);
                cursor = None;
            }
        }
    }
    finish_contour(&mut contours, &mut current);
    contours
}

fn close_contour(contour: &mut Vec<Point>) {
    if let Some(first) = contour.first().copied() {
        if contour.last().copied() != Some(first) {
            contour.push(first);
        }
    }
}

fn finish_contour(contours: &mut Vec<Vec<Point>>, current: &mut Vec<Point>) {
    close_contour(current);
    if current.len() >= 4 {
        contours.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn project(point: Point, viewport: BBox, scale: f64) -> Point {
    Point::new(
        (point.x - viewport.x0) * scale,
        (point.y - viewport.top) * scale,
    )
}

fn cubic_point(start: Point, c1: Point, c2: Point, end: Point, t: f64) -> Point {
    let one_minus_t = 1.0 - t;
    let a = one_minus_t * one_minus_t * one_minus_t;
    let b = 3.0 * one_minus_t * one_minus_t * t;
    let c = 3.0 * one_minus_t * t * t;
    let d = t * t * t;
    Point::new(
        a * start.x + b * c1.x + c * c2.x + d * end.x,
        a * start.y + b * c1.y + c * c2.y + d * end.y,
    )
}
