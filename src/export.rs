use std::{fmt, fs, io::Cursor, path::Path};

use anyhow::{Context, Result};
use base64::{Engine, prelude::BASE64_STANDARD};
use clap::ValueEnum;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::canvas::{
    BaseSource, DrawElement, DrawStyle, DrawingCanvas, Point, RenderSizing, annotation_font_bytes,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Png,
    Svg,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }
}

impl fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportSize {
    Original,
    Canvas,
}

impl fmt::Display for ExportSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Original => f.write_str("original"),
            Self::Canvas => f.write_str("canvas"),
        }
    }
}

pub fn save(
    path: &Path,
    format: ExportFormat,
    size: ExportSize,
    canvas: &DrawingCanvas,
) -> Result<()> {
    match format {
        ExportFormat::Png => save_png(path, &render_image(size, canvas)),
        ExportFormat::Svg => save_svg(path, size, canvas),
    }
}

pub fn render_image(size: ExportSize, canvas: &DrawingCanvas) -> RgbaImage {
    match size {
        ExportSize::Original if matches!(canvas.source(), BaseSource::Image(_)) => {
            canvas.render_original_export()
        }
        ExportSize::Original | ExportSize::Canvas => canvas.render_canvas_export(),
    }
}

fn save_png(path: &Path, image: &RgbaImage) -> Result<()> {
    image
        .save(path)
        .with_context(|| format!("failed to write PNG output {}", path.display()))
}

fn save_svg(path: &Path, size: ExportSize, canvas: &DrawingCanvas) -> Result<()> {
    let svg = if canvas.has_redactions() {
        raster_safe_svg(size, canvas)?
    } else {
        vector_svg(size, canvas)?
    };
    fs::write(path, svg).with_context(|| format!("failed to write SVG output {}", path.display()))
}

fn raster_safe_svg(size: ExportSize, canvas: &DrawingCanvas) -> Result<String> {
    let image = render_image(size, canvas);
    let width = image.width();
    let height = image.height();
    let href = png_data_uri(&image)?;
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" overflow="hidden"><image href="{href}" x="0" y="0" width="{width}" height="{height}"/></svg>
"#
    ))
}

fn vector_svg(size: ExportSize, canvas: &DrawingCanvas) -> Result<String> {
    let target = svg_target(size, canvas);
    let base_href = png_data_uri(&target.base)?;
    let font = BASE64_STANDARD.encode(annotation_font_bytes());
    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" overflow="hidden">
<defs><style><![CDATA[
@font-face{{font-family:'KitDraw Noto Sans';src:url(data:font/ttf;base64,{font}) format('truetype');}}
]]></style></defs>
<image href="{base_href}" x="0" y="0" width="{width}" height="{height}"/>
"#,
        width = target.width,
        height = target.height,
    ));

    for element in &target.elements {
        write_svg_element(
            &mut svg,
            element,
            target.width,
            target.height,
            target.sizing,
        );
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

struct SvgTarget {
    width: u32,
    height: u32,
    base: RgbaImage,
    elements: Vec<DrawElement>,
    sizing: RenderSizing,
}

fn svg_target(size: ExportSize, canvas: &DrawingCanvas) -> SvgTarget {
    if matches!(size, ExportSize::Original)
        && let (Some(base), Some(scale)) = (canvas.original_base(), canvas.original_export_scale())
    {
        return SvgTarget {
            width: base.width(),
            height: base.height(),
            base,
            elements: canvas.transformed_elements_for_original(),
            sizing: canvas.preview_sizing().scaled(scale),
        };
    }

    let base = canvas.canvas_base().clone();
    SvgTarget {
        width: base.width(),
        height: base.height(),
        base,
        elements: canvas.elements().to_vec(),
        sizing: canvas.preview_sizing(),
    }
}

fn write_svg_element(
    svg: &mut String,
    element: &DrawElement,
    width: u32,
    height: u32,
    sizing: RenderSizing,
) {
    match element {
        DrawElement::Freehand { points, style } => {
            write_svg_path(svg, points, *style, width, height, sizing, 1.0)
        }
        DrawElement::Highlighter { points, style } => {
            write_svg_path(svg, points, *style, width, height, sizing, 3.2)
        }
        DrawElement::Rectangle { start, end, style } => {
            let (x1, y1) = svg_point(*start, width, height);
            let (x2, y2) = svg_point(*end, width, height);
            let x = x1.min(x2);
            let y = y1.min(y2);
            let rect_width = (x1 - x2).abs();
            let rect_height = (y1 - y2).abs();
            svg.push_str(&format!(
                r#"<rect x="{x:.2}" y="{y:.2}" width="{rect_width:.2}" height="{rect_height:.2}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-opacity="{:.3}" stroke-linejoin="round"/>
"#,
                svg_color(style.color),
                sizing.stroke_radius_for_style(*style) * 2.0,
                style.opacity.clamp(0.0, 1.0)
            ));
        }
        DrawElement::Ellipse { start, end, style } => {
            let (x1, y1) = svg_point(*start, width, height);
            let (x2, y2) = svg_point(*end, width, height);
            let cx = (x1 + x2) * 0.5;
            let cy = (y1 + y2) * 0.5;
            let rx = (x1 - x2).abs() * 0.5;
            let ry = (y1 - y2).abs() * 0.5;
            svg.push_str(&format!(
                r#"<ellipse cx="{cx:.2}" cy="{cy:.2}" rx="{rx:.2}" ry="{ry:.2}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-opacity="{:.3}"/>
"#,
                svg_color(style.color),
                sizing.stroke_radius_for_style(*style) * 2.0,
                style.opacity.clamp(0.0, 1.0)
            ));
        }
        DrawElement::Arrow { start, end, style } => {
            write_svg_arrow(svg, *start, *end, *style, width, height, sizing);
        }
        DrawElement::Text {
            position,
            text,
            style,
        } => {
            let (x, y) = svg_point(*position, width, height);
            svg.push_str(&format!(
                r#"<text x="{x:.2}" y="{y:.2}" fill="{}" fill-opacity="{:.3}" font-family="KitDraw Noto Sans, sans-serif" font-size="{:.2}" dominant-baseline="hanging">{}</text>
"#,
                svg_color(style.color),
                style.opacity.clamp(0.0, 1.0),
                sizing.text_size_for_style(*style),
                escape_xml(text)
            ));
        }
        DrawElement::Redaction { start, end } => {
            let (x1, y1) = svg_point(*start, width, height);
            let (x2, y2) = svg_point(*end, width, height);
            svg.push_str(&format!(
                r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="#000000"/>
            "##,
                x1.min(x2),
                y1.min(y2),
                (x1 - x2).abs(),
                (y1 - y2).abs()
            ));
        }
    }
}

fn write_svg_path(
    svg: &mut String,
    points: &[Point],
    style: DrawStyle,
    width: u32,
    height: u32,
    sizing: RenderSizing,
    width_multiplier: f32,
) {
    let Some(first) = points.first().copied() else {
        return;
    };
    if points.len() == 1 {
        let (cx, cy) = svg_point(first, width, height);
        let radius = sizing.stroke_radius_for_style(style) * width_multiplier;
        svg.push_str(&format!(
            r#"<circle cx="{cx:.2}" cy="{cy:.2}" r="{radius:.2}" fill="{}" fill-opacity="{:.3}"/>
"#,
            svg_color(style.color),
            style.opacity.clamp(0.0, 1.0)
        ));
        return;
    }

    let mut data = String::new();
    let (x, y) = svg_point(first, width, height);
    data.push_str(&format!("M {x:.2} {y:.2}"));
    for point in &points[1..] {
        let (x, y) = svg_point(*point, width, height);
        data.push_str(&format!(" L {x:.2} {y:.2}"));
    }
    svg.push_str(&format!(
        r#"<path d="{data}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-opacity="{:.3}" stroke-linecap="round" stroke-linejoin="round"/>
"#,
        svg_color(style.color),
        sizing.stroke_radius_for_style(style) * width_multiplier * 2.0,
        style.opacity.clamp(0.0, 1.0)
    ));
}

fn write_svg_arrow(
    svg: &mut String,
    start: Point,
    end: Point,
    style: DrawStyle,
    width: u32,
    height: u32,
    sizing: RenderSizing,
) {
    let (start_x, start_y) = svg_point(start, width, height);
    let (end_x, end_y) = svg_point(end, width, height);
    let radius = sizing.stroke_radius_for_style(style);
    svg.push_str(&format!(
        r#"<line x1="{start_x:.2}" y1="{start_y:.2}" x2="{end_x:.2}" y2="{end_y:.2}" stroke="{}" stroke-width="{:.2}" stroke-opacity="{:.3}" stroke-linecap="round"/>
"#,
        svg_color(style.color),
        radius * 2.0,
        style.opacity.clamp(0.0, 1.0)
    ));

    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let length = dx.hypot(dy);
    if length <= 0.5 {
        return;
    }
    let ux = dx / length;
    let uy = dy / length;
    let px = -uy;
    let py = ux;
    let head_len = (radius * 7.0).max(8.0);
    let head_width = (radius * 4.5).max(5.0);
    let base_x = end_x - ux * head_len;
    let base_y = end_y - uy * head_len;
    let left = (base_x + px * head_width, base_y + py * head_width);
    let right = (base_x - px * head_width, base_y - py * head_width);
    svg.push_str(&format!(
        r#"<polygon points="{end_x:.2},{end_y:.2} {:.2},{:.2} {:.2},{:.2}" fill="{}" fill-opacity="{:.3}"/>
"#,
        left.0,
        left.1,
        right.0,
        right.1,
        svg_color(style.color),
        style.opacity.clamp(0.0, 1.0)
    ));
}

fn svg_point(point: Point, width: u32, height: u32) -> (f32, f32) {
    (point.x() * width as f32, point.y() * height as f32)
}

fn svg_color(color: Rgba<u8>) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn png_data_uri(image: &RgbaImage) -> Result<String> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut cursor, image::ImageFormat::Png)
        .context("failed to encode embedded PNG")?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(cursor.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{DynamicImage, Rgba, RgbaImage};

    use super::*;
    use crate::{
        canvas::{DrawStyle, DrawingTool, Point, WidthPreset},
        terminal::TerminalMetrics,
        theme::ThemeMode,
    };

    fn image_canvas() -> DrawingCanvas {
        let image =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(200, 100, Rgba([240, 240, 240, 255])));
        DrawingCanvas::new(
            TerminalMetrics::from_dimensions(10, 10, 100, 100),
            BaseSource::Image(image),
            ThemeMode::Dark,
        )
    }

    fn style(color: Rgba<u8>) -> DrawStyle {
        DrawStyle::new(color, WidthPreset::Medium)
    }

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kitdraw_export_{}_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test"),
            name
        ))
    }

    #[test]
    fn original_export_uses_input_dimensions() {
        let mut canvas = image_canvas();
        canvas.begin_element(
            DrawingTool::Rectangle,
            Point::new(0.25, 0.35),
            style(Rgba([255, 0, 0, 255])),
        );
        canvas.extend_current(Point::new(0.75, 0.65));
        canvas.finish_current();

        let image = render_image(ExportSize::Original, &canvas);

        assert_eq!(image.dimensions(), (200, 100));
    }

    #[test]
    fn original_export_clips_preview_letterbox_annotations() {
        let mut canvas = image_canvas();
        canvas.begin_element(
            DrawingTool::Redaction,
            Point::new(0.1, 0.05),
            style(Rgba([255, 0, 0, 255])),
        );
        canvas.extend_current(Point::new(0.2, 0.15));
        canvas.finish_current();

        let image = render_image(ExportSize::Original, &canvas);

        assert_eq!(*image.get_pixel(20, 5), Rgba([240, 240, 240, 255]));
    }

    #[test]
    fn vector_svg_contains_editable_text_and_escaped_content() {
        let mut canvas = image_canvas();
        canvas.add_text(
            Point::new(0.3, 0.4),
            String::from("A&B <1>"),
            style(Rgba([30, 100, 255, 255])),
        );
        let path = temp_file("vector.svg");
        save(&path, ExportFormat::Svg, ExportSize::Original, &canvas).unwrap();
        let svg = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert!(svg.contains("@font-face"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("A&amp;B &lt;1&gt;"));
    }

    #[test]
    fn redacted_svg_is_raster_safe() {
        let mut canvas = image_canvas();
        canvas.add_text(
            Point::new(0.3, 0.4),
            String::from("secret literal"),
            style(Rgba([255, 0, 0, 255])),
        );
        canvas.begin_element(
            DrawingTool::Redaction,
            Point::new(0.25, 0.35),
            style(Rgba([255, 0, 0, 255])),
        );
        canvas.extend_current(Point::new(0.75, 0.65));
        canvas.finish_current();
        let path = temp_file("safe.svg");
        save(&path, ExportFormat::Svg, ExportSize::Original, &canvas).unwrap();
        let svg = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert!(svg.contains("<image"));
        assert!(!svg.contains("<text"));
        assert!(!svg.contains("secret literal"));
        assert!(!svg.contains("@font-face"));
    }
}
