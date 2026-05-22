use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, imageops::FilterType};

use crate::{terminal::TerminalMetrics, theme::ThemeMode};

#[derive(Debug, Clone)]
pub struct DrawingCanvas {
    metrics: TerminalMetrics,
    source: BaseSource,
    base: RgbaImage,
    committed: RgbaImage,
    elements: Vec<DrawElement>,
    current: Option<DrawElement>,
    theme: ThemeMode,
}

#[derive(Debug, Clone)]
pub enum BaseSource {
    Blank,
    Image(DynamicImage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingTool {
    Freehand,
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawElement {
    Freehand {
        points: Vec<Point>,
        color: Rgba<u8>,
    },
    Rectangle {
        start: Point,
        end: Point,
        color: Rgba<u8>,
    },
    Ellipse {
        start: Point,
        end: Point,
        color: Rgba<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    x: f32,
    y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }
    }
}

impl DrawingCanvas {
    pub fn new(metrics: TerminalMetrics, source: BaseSource, theme: ThemeMode) -> Self {
        let base = render_base(
            &source,
            metrics.width_px,
            metrics.height_px,
            theme.background(),
        );
        let committed = base.clone();
        Self {
            metrics,
            source,
            base,
            committed,
            elements: Vec::new(),
            current: None,
            theme,
        }
    }

    pub fn blank(metrics: TerminalMetrics, theme: ThemeMode) -> Self {
        Self::new(metrics, BaseSource::Blank, theme)
    }

    pub fn resize(&mut self, metrics: TerminalMetrics) {
        self.metrics = metrics;
        self.base = render_base(
            &self.source,
            metrics.width_px,
            metrics.height_px,
            self.theme.background(),
        );
        self.rebuild_committed();
    }

    pub fn begin_element(&mut self, tool: DrawingTool, point: Point, color: Rgba<u8>) {
        self.current = Some(match tool {
            DrawingTool::Freehand => DrawElement::Freehand {
                points: vec![point],
                color,
            },
            DrawingTool::Rectangle => DrawElement::Rectangle {
                start: point,
                end: point,
                color,
            },
            DrawingTool::Ellipse => DrawElement::Ellipse {
                start: point,
                end: point,
                color,
            },
        });
    }

    pub fn extend_current(&mut self, point: Point) {
        match self.current.as_mut() {
            Some(DrawElement::Freehand { points, .. }) => {
                if points.last().copied() != Some(point) {
                    points.push(point);
                }
            }
            Some(DrawElement::Rectangle { end, .. } | DrawElement::Ellipse { end, .. }) => {
                *end = point
            }
            None => self.begin_stroke(point),
        }
    }

    pub fn finish_current(&mut self) -> bool {
        if let Some(element) = self.current.take() {
            if element.is_empty() {
                return false;
            }
            draw_element(
                &mut self.committed,
                &element,
                self.metrics.brush_radius_px(),
            );
            self.elements.push(element);
            return true;
        }
        false
    }

    pub fn cancel_current(&mut self) -> bool {
        self.current.take().is_some()
    }

    pub fn default_stroke_color(&self) -> Rgba<u8> {
        self.theme.stroke()
    }

    pub fn begin_stroke(&mut self, point: Point) {
        self.begin_element(DrawingTool::Freehand, point, self.theme.stroke());
    }

    pub fn undo(&mut self) -> bool {
        self.current = None;
        let did_undo = self.elements.pop().is_some();
        if did_undo {
            self.rebuild_committed();
        }
        did_undo
    }

    pub fn clear(&mut self) -> bool {
        self.current = None;
        let had_strokes = !self.elements.is_empty();
        self.elements.clear();
        self.committed = self.base.clone();
        had_strokes
    }

    pub fn render(&self) -> RgbaImage {
        let mut image = self.committed.clone();
        if let Some(element) = &self.current {
            draw_element(&mut image, element, self.metrics.brush_radius_px());
        }
        image
    }

    pub fn point_for_mouse_cell(&self, column: u16, row: u16) -> Point {
        let x_px = (f32::from(column) + 0.5) * self.metrics.cell_width_px;
        let y_px = (f32::from(row) + 0.5) * self.metrics.cell_height_px;
        self.point_for_pixel(x_px, y_px)
    }

    pub fn point_for_mouse_pixel(&self, column: u16, row: u16) -> Point {
        Point::new(
            f32::from(column) / self.metrics.display_width_px as f32,
            f32::from(row) / self.metrics.display_height_px as f32,
        )
    }

    fn point_for_pixel(&self, x_px: f32, y_px: f32) -> Point {
        Point::new(
            x_px / self.metrics.width_px as f32,
            y_px / self.metrics.height_px as f32,
        )
    }

    pub fn metrics(&self) -> TerminalMetrics {
        self.metrics
    }

    fn rebuild_committed(&mut self) {
        self.committed = self.base.clone();
        for element in &self.elements {
            draw_element(&mut self.committed, element, self.metrics.brush_radius_px());
        }
    }

    #[cfg(test)]
    fn stroke_count(&self) -> usize {
        self.elements.len()
    }
}

impl DrawElement {
    fn is_empty(&self) -> bool {
        match self {
            Self::Freehand { points, .. } => points.is_empty(),
            Self::Rectangle { .. } | Self::Ellipse { .. } => false,
        }
    }
}

fn render_base(source: &BaseSource, width: u32, height: u32, background: Rgba<u8>) -> RgbaImage {
    let mut base = RgbaImage::from_pixel(width.max(1), height.max(1), background);
    let BaseSource::Image(image) = source else {
        return base;
    };
    let (fit_width, fit_height) = fit_dimensions(image.dimensions(), (base.width(), base.height()));
    let resized = image
        .resize_exact(fit_width, fit_height, FilterType::Lanczos3)
        .to_rgba8();
    let x = ((base.width() - fit_width) / 2) as i32;
    let y = ((base.height() - fit_height) / 2) as i32;
    overlay(&mut base, x, y, &resized);
    base
}

fn fit_dimensions((src_w, src_h): (u32, u32), (dst_w, dst_h): (u32, u32)) -> (u32, u32) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return (1, 1);
    }
    let scale = (dst_w as f32 / src_w as f32).min(dst_h as f32 / src_h as f32);
    let width = (src_w as f32 * scale).round().clamp(1.0, dst_w as f32) as u32;
    let height = (src_h as f32 * scale).round().clamp(1.0, dst_h as f32) as u32;
    (width, height)
}

fn draw_element(image: &mut RgbaImage, element: &DrawElement, radius: f32) {
    match element {
        DrawElement::Freehand { points, color } => draw_freehand(image, points, *color, radius),
        DrawElement::Rectangle { start, end, color } => {
            draw_rectangle_outline(image, *start, *end, *color, radius)
        }
        DrawElement::Ellipse { start, end, color } => {
            draw_ellipse_outline(image, *start, *end, *color, radius)
        }
    }
}

fn draw_freehand(image: &mut RgbaImage, stroke_points: &[Point], color: Rgba<u8>, radius: f32) {
    let points = curve_points(stroke_points, image.width(), image.height(), radius);
    let Some(first) = points.first().copied() else {
        return;
    };
    if points.len() == 1 {
        stamp_circle(image, first, color, radius);
        return;
    }
    for points in points.windows(2) {
        draw_segment(image, points[0], points[1], color, radius);
    }
}

fn draw_rectangle_outline(
    image: &mut RgbaImage,
    start: Point,
    end: Point,
    color: Rgba<u8>,
    radius: f32,
) {
    let left = start.x.min(end.x);
    let right = start.x.max(end.x);
    let top = start.y.min(end.y);
    let bottom = start.y.max(end.y);
    let top_left = Point::new(left, top);
    let top_right = Point::new(right, top);
    let bottom_right = Point::new(right, bottom);
    let bottom_left = Point::new(left, bottom);

    draw_segment(image, top_left, top_right, color, radius);
    draw_segment(image, top_right, bottom_right, color, radius);
    draw_segment(image, bottom_right, bottom_left, color, radius);
    draw_segment(image, bottom_left, top_left, color, radius);
}

fn draw_ellipse_outline(
    image: &mut RgbaImage,
    start: Point,
    end: Point,
    color: Rgba<u8>,
    radius: f32,
) {
    let (start_x, start_y) = point_to_pixel(image, start);
    let (end_x, end_y) = point_to_pixel(image, end);
    let radius_x = (end_x - start_x).abs() * 0.5;
    let radius_y = (end_y - start_y).abs() * 0.5;
    if radius_x <= 0.5 || radius_y <= 0.5 {
        draw_segment(image, start, end, color, radius);
        return;
    }

    let center_x = (start_x + end_x) * 0.5;
    let center_y = (start_y + end_y) * 0.5;
    let circumference = std::f32::consts::PI
        * (3.0 * (radius_x + radius_y)
            - ((3.0 * radius_x + radius_y) * (radius_x + 3.0 * radius_y)).sqrt());
    let samples = (circumference / (radius * 0.65).max(1.0))
        .ceil()
        .clamp(16.0, 240.0) as u32;
    let mut previous = None;
    let mut first = None;

    for step in 0..samples {
        let theta = std::f32::consts::TAU * step as f32 / samples as f32;
        let point = point_from_dimensions_pixel(
            image.width(),
            image.height(),
            center_x + radius_x * theta.cos(),
            center_y + radius_y * theta.sin(),
        );
        if first.is_none() {
            first = Some(point);
        }
        if let Some(previous) = previous {
            draw_segment(image, previous, point, color, radius);
        }
        previous = Some(point);
    }

    if let (Some(previous), Some(first)) = (previous, first) {
        draw_segment(image, previous, first, color, radius);
    }
}

fn curve_points(points: &[Point], width: u32, height: u32, radius: f32) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut curved = Vec::with_capacity(points.len() * 4);
    for idx in 0..points.len() - 1 {
        let p0 = point_to_dimensions_pixel(width, height, points[idx.saturating_sub(1)]);
        let p1 = point_to_dimensions_pixel(width, height, points[idx]);
        let p2 = point_to_dimensions_pixel(width, height, points[idx + 1]);
        let p3 = point_to_dimensions_pixel(width, height, points[(idx + 2).min(points.len() - 1)]);
        let distance = (p2.0 - p1.0).hypot(p2.1 - p1.1);
        let samples = (distance / (radius * 0.65).max(1.0))
            .ceil()
            .clamp(2.0, 32.0) as u32;
        let first_sample = if idx == 0 { 0 } else { 1 };
        for step in first_sample..=samples {
            let t = step as f32 / samples as f32;
            let (x, y) = catmull_rom(p0, p1, p2, p3, t);
            curved.push(point_from_dimensions_pixel(width, height, x, y));
        }
    }
    curved
}

fn catmull_rom(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    (
        0.5 * ((2.0 * p1.0)
            + (-p0.0 + p2.0) * t
            + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
            + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3),
        0.5 * ((2.0 * p1.1)
            + (-p0.1 + p2.1) * t
            + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
            + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3),
    )
}

fn draw_segment(image: &mut RgbaImage, start: Point, end: Point, color: Rgba<u8>, radius: f32) {
    let (start_x, start_y) = point_to_pixel(image, start);
    let (end_x, end_y) = point_to_pixel(image, end);
    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let distance = dx.hypot(dy);
    let steps = (distance / (radius * 0.5).max(1.0)).ceil().max(1.0) as u32;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        stamp_circle_at(image, start_x + dx * t, start_y + dy * t, color, radius);
    }
}

fn stamp_circle(image: &mut RgbaImage, point: Point, color: Rgba<u8>, radius: f32) {
    let (x, y) = point_to_pixel(image, point);
    stamp_circle_at(image, x, y, color, radius);
}

fn stamp_circle_at(image: &mut RgbaImage, x: f32, y: f32, color: Rgba<u8>, radius: f32) {
    let radius = radius.max(1.0);
    let min_x = (x - radius).floor() as i32;
    let max_x = (x + radius).ceil() as i32;
    let min_y = (y - radius).floor() as i32;
    let max_y = (y + radius).ceil() as i32;
    let radius_squared = radius * radius;

    for yy in min_y..=max_y {
        for xx in min_x..=max_x {
            if xx < 0 || yy < 0 || xx >= image.width() as i32 || yy >= image.height() as i32 {
                continue;
            }
            let px = xx as f32 + 0.5;
            let py = yy as f32 + 0.5;
            if (px - x).powi(2) + (py - y).powi(2) <= radius_squared {
                image.put_pixel(xx as u32, yy as u32, color);
            }
        }
    }
}

fn point_to_pixel(image: &RgbaImage, point: Point) -> (f32, f32) {
    point_to_dimensions_pixel(image.width(), image.height(), point)
}

fn point_to_dimensions_pixel(width: u32, height: u32, point: Point) -> (f32, f32) {
    (
        point.x * width.saturating_sub(1) as f32,
        point.y * height.saturating_sub(1) as f32,
    )
}

fn point_from_dimensions_pixel(width: u32, height: u32, x: f32, y: f32) -> Point {
    Point::new(
        x / width.saturating_sub(1).max(1) as f32,
        y / height.saturating_sub(1).max(1) as f32,
    )
}

fn overlay(dst: &mut RgbaImage, x: i32, y: i32, src: &RgbaImage) {
    for sy in 0..src.height() {
        for sx in 0..src.width() {
            let dx = x + sx as i32;
            let dy = y + sy as i32;
            if dx >= 0 && dy >= 0 && dx < dst.width() as i32 && dy < dst.height() as i32 {
                blend_pixel(dst, dx as u32, dy as u32, *src.get_pixel(sx, sy));
            }
        }
    }
}

fn blend_pixel(dst: &mut RgbaImage, x: u32, y: u32, src: Rgba<u8>) {
    let alpha = src[3] as f32 / 255.0;
    if alpha <= 0.0 {
        return;
    }
    if alpha >= 1.0 {
        dst.put_pixel(x, y, src);
        return;
    }
    let mut out = *dst.get_pixel(x, y);
    for channel in 0..3 {
        out[channel] =
            ((src[channel] as f32 * alpha) + (out[channel] as f32 * (1.0 - alpha))).round() as u8;
    }
    out[3] = 255;
    dst.put_pixel(x, y, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> TerminalMetrics {
        TerminalMetrics::from_dimensions(10, 5, 100, 50)
    }

    #[test]
    fn blank_canvas_uses_theme_background() {
        let canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        let image = canvas.render();
        assert_eq!(*image.get_pixel(0, 0), Rgba([0, 0, 0, 255]));

        let canvas = DrawingCanvas::blank(metrics(), ThemeMode::Light);
        let image = canvas.render();
        assert_eq!(*image.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn fit_dimensions_preserve_aspect() {
        assert_eq!(fit_dimensions((400, 200), (100, 100)), (100, 50));
        assert_eq!(fit_dimensions((200, 400), (100, 100)), (50, 100));
    }

    #[test]
    fn draws_continuous_stroke() {
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        canvas.begin_stroke(Point::new(0.1, 0.5));
        canvas.extend_current(Point::new(0.9, 0.5));
        canvas.finish_current();
        let image = canvas.render();

        for x in 15..85 {
            assert_eq!(*image.get_pixel(x, 25), Rgba([255, 255, 255, 255]));
        }
    }

    #[test]
    fn draws_rectangle_outline_without_filling_center() {
        let red = Rgba([255, 0, 0, 255]);
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        canvas.begin_element(DrawingTool::Rectangle, Point::new(0.2, 0.2), red);
        canvas.extend_current(Point::new(0.8, 0.8));
        canvas.finish_current();
        let image = canvas.render();

        assert_eq!(*image.get_pixel(50, 10), red);
        assert_eq!(*image.get_pixel(20, 25), red);
        assert_eq!(*image.get_pixel(50, 25), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn draws_ellipse_outline_without_filling_center() {
        let green = Rgba([0, 180, 80, 255]);
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        canvas.begin_element(DrawingTool::Ellipse, Point::new(0.2, 0.2), green);
        canvas.extend_current(Point::new(0.8, 0.8));
        canvas.finish_current();
        let image = canvas.render();

        assert_eq!(*image.get_pixel(50, 10), green);
        assert_eq!(*image.get_pixel(50, 25), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn committed_elements_keep_their_original_colors_after_resize() {
        let red = Rgba([255, 0, 0, 255]);
        let blue = Rgba([30, 100, 255, 255]);
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        canvas.begin_element(DrawingTool::Freehand, Point::new(0.1, 0.2), red);
        canvas.extend_current(Point::new(0.4, 0.2));
        canvas.finish_current();
        canvas.begin_element(DrawingTool::Rectangle, Point::new(0.6, 0.6), blue);
        canvas.extend_current(Point::new(0.9, 0.9));
        canvas.finish_current();

        canvas.resize(metrics());
        let image = canvas.render();

        assert_eq!(*image.get_pixel(20, 10), red);
        assert_eq!(*image.get_pixel(60, 30), blue);
    }

    #[test]
    fn undo_removes_completed_strokes_many_times() {
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        for x in [0.2, 0.4, 0.6] {
            canvas.begin_stroke(Point::new(x, 0.5));
            canvas.finish_current();
        }

        assert_eq!(canvas.stroke_count(), 3);
        assert!(canvas.undo());
        assert!(canvas.undo());
        assert!(canvas.undo());
        assert!(!canvas.undo());
        assert_eq!(canvas.stroke_count(), 0);
    }

    #[test]
    fn clear_removes_strokes_and_preserves_base() {
        let mut canvas = DrawingCanvas::blank(metrics(), ThemeMode::Light);
        canvas.begin_stroke(Point::new(0.5, 0.5));
        canvas.finish_current();
        assert!(canvas.clear());
        assert!(!canvas.undo());

        let image = canvas.render();
        assert_eq!(*image.get_pixel(50, 25), Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn mouse_cells_map_to_normalized_points() {
        let canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        assert_eq!(canvas.point_for_mouse_cell(0, 0), Point::new(0.05, 0.1));
        assert_eq!(canvas.point_for_mouse_cell(9, 4), Point::new(0.95, 0.9));
    }

    #[test]
    fn mouse_pixels_map_to_normalized_points() {
        let canvas = DrawingCanvas::blank(metrics(), ThemeMode::Dark);
        assert_eq!(canvas.point_for_mouse_pixel(0, 0), Point::new(0.0, 0.0));
        assert_eq!(canvas.point_for_mouse_pixel(50, 25), Point::new(0.5, 0.5));
        assert_eq!(canvas.point_for_mouse_pixel(100, 50), Point::new(1.0, 1.0));
    }

    #[test]
    fn curve_points_preserve_cursor_trail_points() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.0),
            Point::new(0.1, 0.1),
            Point::new(0.2, 0.1),
            Point::new(0.2, 0.2),
            Point::new(0.3, 0.2),
            Point::new(0.3, 0.3),
        ];
        let curved = curve_points(&points, 100, 100, 2.0);
        assert!(curved.len() > points.len());
        assert!(points_are_close(
            curved.first().copied().unwrap(),
            points[0]
        ));
        assert!(points_are_close(
            curved.last().copied().unwrap(),
            points[points.len() - 1]
        ));
        for point in points {
            assert!(curved.iter().any(|curved| points_are_close(*curved, point)));
        }
    }

    fn points_are_close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001
    }
}
