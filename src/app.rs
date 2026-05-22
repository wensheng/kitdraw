use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::MoveTo,
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor},
    terminal::{Clear, ClearType},
};
use image::Rgba;

use crate::{
    args::{CellPixels, ensure_output_path},
    canvas::{BaseSource, DrawingCanvas, DrawingTool, Point},
    kitty,
    terminal::{TerminalLayout, TerminalSession},
    theme::ThemeMode,
};

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MIN_STATUS_HIT_SLOP_PX: u16 = 4;

pub struct AppConfig {
    pub input_image: Option<PathBuf>,
    pub output: PathBuf,
    pub theme: ThemeMode,
    pub fallback_cell_px: CellPixels,
    pub resolution_scale: f32,
}

#[derive(Debug, Clone)]
struct AppState {
    tool: DrawingTool,
    color: Rgba<u8>,
    input_mode: bool,
    color_buffer: String,
    message: String,
}

impl AppState {
    fn new(color: Rgba<u8>) -> Self {
        Self {
            tool: DrawingTool::Freehand,
            color,
            input_mode: false,
            color_buffer: String::new(),
            message: String::from("Ready"),
        }
    }

    fn set_tool(&mut self, tool: DrawingTool) {
        self.tool = tool;
        self.message = format!("Tool: {}", tool_label(tool));
    }

    fn begin_color_input(&mut self) {
        self.input_mode = true;
        self.color_buffer.clear();
        self.message = String::from("Enter color");
    }

    fn cancel_color_input(&mut self) {
        self.input_mode = false;
        self.color_buffer.clear();
        self.message = String::from("Color unchanged");
    }

    fn apply_color_input(&mut self) {
        match parse_color(&self.color_buffer) {
            Some(color) => {
                self.color = color;
                self.input_mode = false;
                self.message = format!("Color: {}", color_to_hex(color));
                self.color_buffer.clear();
            }
            None => {
                let value = self.color_buffer.trim();
                self.message = if value.is_empty() {
                    String::from("Enter a color name or hex value")
                } else {
                    format!("Unknown color: {value}")
                };
            }
        }
    }

    fn set_color(&mut self, color: Rgba<u8>, name: &str) {
        self.color = color;
        self.input_mode = false;
        self.color_buffer.clear();
        self.message = format!("Color: {name}");
    }
}

#[derive(Debug, Clone, Copy)]
struct PaletteColor {
    name: &'static str,
    color: Rgba<u8>,
}

const PALETTE: [PaletteColor; 9] = [
    PaletteColor {
        name: "black",
        color: Rgba([0, 0, 0, 255]),
    },
    PaletteColor {
        name: "white",
        color: Rgba([255, 255, 255, 255]),
    },
    PaletteColor {
        name: "red",
        color: Rgba([255, 0, 0, 255]),
    },
    PaletteColor {
        name: "orange",
        color: Rgba([255, 128, 0, 255]),
    },
    PaletteColor {
        name: "yellow",
        color: Rgba([255, 221, 0, 255]),
    },
    PaletteColor {
        name: "green",
        color: Rgba([0, 180, 80, 255]),
    },
    PaletteColor {
        name: "cyan",
        color: Rgba([0, 190, 220, 255]),
    },
    PaletteColor {
        name: "blue",
        color: Rgba([30, 100, 255, 255]),
    },
    PaletteColor {
        name: "purple",
        color: Rgba([160, 80, 220, 255]),
    },
];

pub fn run(config: AppConfig) -> Result<()> {
    ensure_output_path(&config.output)?;
    let source = match config.input_image.as_deref() {
        Some(path) => {
            if !path.exists() {
                anyhow::bail!("input image not found: {}", path.display());
            }
            BaseSource::Image(
                image::open(path)
                    .with_context(|| format!("failed to load image {}", path.display()))?,
            )
        }
        None => BaseSource::Blank,
    };

    let session = TerminalSession::enter()?;
    let layout = TerminalLayout::current(config.fallback_cell_px, config.resolution_scale);
    let mut canvas = match source {
        BaseSource::Blank => DrawingCanvas::blank(layout.canvas, config.theme),
        BaseSource::Image(image) => {
            DrawingCanvas::new(layout.canvas, BaseSource::Image(image), config.theme)
        }
    };
    let mut state = AppState::new(canvas.default_stroke_color());
    let final_image = run_event_loop(
        &mut canvas,
        &mut state,
        layout,
        config.fallback_cell_px,
        config.resolution_scale,
    )?;
    drop(session);

    save_png(&config.output, &final_image)?;
    println!("Saved {}", config.output.display());
    Ok(())
}

fn run_event_loop(
    canvas: &mut DrawingCanvas,
    state: &mut AppState,
    mut layout: TerminalLayout,
    fallback_cell_px: CellPixels,
    resolution_scale: f32,
) -> Result<image::RgbaImage> {
    let mut stdout = io::stdout().lock();
    render_to_terminal(&mut stdout, canvas, state, layout)?;
    let mut last_render = Instant::now();
    let mut dirty = false;
    let mut mouse_mapper = MouseMapper::new();

    loop {
        let wait = if dirty {
            FRAME_INTERVAL
                .checked_sub(last_render.elapsed())
                .unwrap_or(Duration::ZERO)
        } else {
            EVENT_POLL_INTERVAL
        };

        if event::poll(wait)? {
            loop {
                match event::read()? {
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        if handle_key(key, canvas, state) {
                            return Ok(canvas.render());
                        }
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        if handle_mouse(mouse, canvas, state, layout, &mut mouse_mapper) {
                            dirty = true;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        layout = TerminalLayout::from_cells(
                            cols,
                            rows,
                            fallback_cell_px,
                            resolution_scale,
                        );
                        canvas.resize(layout.canvas);
                        mouse_mapper = MouseMapper::new();
                        dirty = true;
                    }
                    _ => {}
                }

                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        if dirty && last_render.elapsed() >= FRAME_INTERVAL {
            render_to_terminal(&mut stdout, canvas, state, layout)?;
            last_render = Instant::now();
            dirty = false;
        }
    }
}

fn handle_key(key: KeyEvent, canvas: &mut DrawingCanvas, state: &mut AppState) -> bool {
    if state.input_mode {
        return handle_color_input_key(key, state);
    }

    match key.code {
        KeyCode::Esc => true,
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char('z') => {
            canvas.undo();
            false
        }
        KeyCode::Char('f') => {
            canvas.cancel_current();
            state.set_tool(DrawingTool::Freehand);
            false
        }
        KeyCode::Char('r') => {
            canvas.cancel_current();
            state.set_tool(DrawingTool::Rectangle);
            false
        }
        KeyCode::Char('e') => {
            canvas.cancel_current();
            state.set_tool(DrawingTool::Ellipse);
            false
        }
        KeyCode::Char('c') => {
            state.begin_color_input();
            false
        }
        KeyCode::Char('C') => {
            canvas.clear();
            state.message = String::from("Drawing layer cleared");
            false
        }
        _ => false,
    }
}

fn handle_color_input_key(key: KeyEvent, state: &mut AppState) -> bool {
    if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    match key.code {
        KeyCode::Esc => {
            state.cancel_color_input();
            false
        }
        KeyCode::Enter => {
            state.apply_color_input();
            false
        }
        KeyCode::Backspace => {
            state.color_buffer.pop();
            false
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.color_buffer.push(ch);
            false
        }
        _ => false,
    }
}

fn handle_mouse(
    mouse: MouseEvent,
    canvas: &mut DrawingCanvas,
    state: &mut AppState,
    layout: TerminalLayout,
    mouse_mapper: &mut MouseMapper,
) -> bool {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            match mouse_mapper.target_for_mouse(mouse, layout, canvas) {
                MouseTarget::Canvas(point) => {
                    canvas.begin_element(state.tool, point, state.color);
                    true
                }
                MouseTarget::Status { column } => {
                    if let Some(palette_color) = palette_color_at_column(column, state, layout.cols)
                    {
                        state.set_color(palette_color.color, palette_color.name);
                        true
                    } else {
                        false
                    }
                }
                MouseTarget::Input | MouseTarget::None => false,
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let MouseTarget::Canvas(point) = mouse_mapper.target_for_mouse(mouse, layout, canvas)
            {
                canvas.extend_current(point);
                true
            } else {
                false
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let MouseTarget::Canvas(point) = mouse_mapper.target_for_mouse(mouse, layout, canvas)
            {
                canvas.extend_current(point);
            }
            canvas.finish_current()
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseCoordinateMode {
    Pixel,
    Cell,
}

#[derive(Debug, Clone, Copy)]
struct MouseMapper {
    mode: MouseCoordinateMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MouseTarget {
    Canvas(Point),
    Status { column: u16 },
    Input,
    None,
}

impl MouseMapper {
    fn new() -> Self {
        Self {
            mode: if prefers_pixel_mouse() {
                MouseCoordinateMode::Pixel
            } else {
                MouseCoordinateMode::Cell
            },
        }
    }

    fn target_for_mouse(
        &mut self,
        mouse: MouseEvent,
        layout: TerminalLayout,
        canvas: &DrawingCanvas,
    ) -> MouseTarget {
        if mouse.column >= layout.cols || mouse.row >= layout.rows {
            self.mode = MouseCoordinateMode::Pixel;
        }
        match self.mode {
            MouseCoordinateMode::Pixel => {
                let column = layout.column_for_pixel(mouse.column);
                let row = layout.row_for_pixel(mouse.row);
                if Some(row) == layout.status_row || is_near_status_row(mouse.row, layout) {
                    return MouseTarget::Status { column };
                }
                if Some(row) == layout.input_row {
                    return MouseTarget::Input;
                }
                if u32::from(mouse.row) < canvas.metrics().display_height_px {
                    return MouseTarget::Canvas(
                        canvas.point_for_mouse_pixel(mouse.column, mouse.row),
                    );
                }
                MouseTarget::None
            }
            MouseCoordinateMode::Cell => {
                if mouse.row < canvas.metrics().rows {
                    MouseTarget::Canvas(canvas.point_for_mouse_cell(mouse.column, mouse.row))
                } else if Some(mouse.row) == layout.status_row {
                    MouseTarget::Status {
                        column: mouse.column,
                    }
                } else if Some(mouse.row) == layout.input_row {
                    MouseTarget::Input
                } else {
                    MouseTarget::None
                }
            }
        }
    }
}

fn is_near_status_row(pixel_y: u16, layout: TerminalLayout) -> bool {
    let Some(status_row) = layout.status_row else {
        return false;
    };
    let status_start = (f32::from(status_row) * layout.cell_height_px).floor() as u16;
    let slop = status_hit_slop_px(layout);
    pixel_y >= status_start.saturating_sub(slop) && pixel_y < status_start
}

fn status_hit_slop_px(layout: TerminalLayout) -> u16 {
    (layout.cell_height_px * 0.25)
        .ceil()
        .clamp(f32::from(MIN_STATUS_HIT_SLOP_PX), 10.0) as u16
}

fn prefers_pixel_mouse() -> bool {
    std::env::var("KITTY_WINDOW_ID").is_ok()
        || std::env::var("TERM_PROGRAM")
            .map(|value| value.to_ascii_lowercase().contains("ghostty"))
            .unwrap_or(false)
        || std::env::var("TERM")
            .map(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("kitty") || value.contains("ghostty")
            })
            .unwrap_or(false)
}

fn render_to_terminal<W: Write>(
    writer: &mut W,
    canvas: &DrawingCanvas,
    state: &AppState,
    layout: TerminalLayout,
) -> Result<()> {
    let image = canvas.render();
    let metrics = canvas.metrics();
    queue!(writer, MoveTo(0, 0))?;
    kitty::write_frame(writer, &image, metrics.cols, metrics.rows, true)?;
    render_ui(writer, state, layout)?;
    Ok(())
}

fn render_ui<W: Write>(writer: &mut W, state: &AppState, layout: TerminalLayout) -> Result<()> {
    if let Some(row) = layout.status_row {
        queue!(writer, MoveTo(0, row), Clear(ClearType::CurrentLine))?;
        write_status_row(writer, state, layout.cols)?;
    }
    if let Some(row) = layout.input_row {
        queue!(writer, MoveTo(0, row), Clear(ClearType::CurrentLine))?;
        let text = input_row_text(state);
        queue!(writer, Print(truncate_to_cols(&text, layout.cols)))?;
    }
    writer.flush()?;
    Ok(())
}

fn write_status_row<W: Write>(writer: &mut W, state: &AppState, cols: u16) -> Result<()> {
    let prefix = status_prefix(state);
    let mut used_cols = prefix.chars().count() as u16;
    queue!(writer, Print(truncate_to_cols(&prefix, cols)))?;
    if used_cols >= cols {
        return Ok(());
    }

    for palette_color in PALETTE {
        let swatch_width = palette_swatch_width(palette_color);
        if used_cols.saturating_add(swatch_width) > cols {
            break;
        }
        queue!(
            writer,
            Print(" "),
            SetBackgroundColor(terminal_color(palette_color.color)),
            Print("  "),
            ResetColor,
            Print(format!(" {}", palette_color.name))
        )?;
        used_cols += swatch_width;
    }
    queue!(writer, ResetColor)?;
    Ok(())
}

fn status_prefix(state: &AppState) -> String {
    format!(
        "Tool {}:{} | Color {} | Palette",
        tool_shortcut(state.tool),
        tool_label(state.tool),
        color_to_hex(state.color)
    )
}

fn input_row_text(state: &AppState) -> String {
    if state.input_mode {
        format!("Color> {}  Enter apply, Esc cancel", state.color_buffer)
    } else {
        format!(
            "{} | f freehand r rectangle e ellipse c color C clear z undo q save",
            state.message
        )
    }
}

fn palette_color_at_column(column: u16, state: &AppState, cols: u16) -> Option<PaletteColor> {
    let mut start = status_prefix(state).chars().count() as u16;
    if start >= cols {
        return None;
    }

    for palette_color in PALETTE {
        let width = palette_swatch_width(palette_color);
        let end = start.saturating_add(width);
        if end > cols {
            return None;
        }
        if column >= start && column < end {
            return Some(palette_color);
        }
        start = end;
    }

    None
}

fn palette_swatch_width(palette_color: PaletteColor) -> u16 {
    palette_color.name.len() as u16 + 4
}

fn tool_label(tool: DrawingTool) -> &'static str {
    match tool {
        DrawingTool::Freehand => "freehand",
        DrawingTool::Rectangle => "rectangle",
        DrawingTool::Ellipse => "ellipse",
    }
}

fn tool_shortcut(tool: DrawingTool) -> char {
    match tool {
        DrawingTool::Freehand => 'f',
        DrawingTool::Rectangle => 'r',
        DrawingTool::Ellipse => 'e',
    }
}

fn parse_color(value: &str) -> Option<Rgba<u8>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    match value.to_ascii_lowercase().as_str() {
        "black" => Some(Rgba([0, 0, 0, 255])),
        "white" => Some(Rgba([255, 255, 255, 255])),
        "red" => Some(Rgba([255, 0, 0, 255])),
        "orange" => Some(Rgba([255, 128, 0, 255])),
        "yellow" => Some(Rgba([255, 221, 0, 255])),
        "green" => Some(Rgba([0, 180, 80, 255])),
        "cyan" => Some(Rgba([0, 190, 220, 255])),
        "blue" => Some(Rgba([30, 100, 255, 255])),
        "purple" => Some(Rgba([160, 80, 220, 255])),
        "pink" => Some(Rgba([255, 96, 170, 255])),
        "magenta" => Some(Rgba([220, 0, 220, 255])),
        "gray" | "grey" => Some(Rgba([128, 128, 128, 255])),
        _ => None,
    }
}

fn parse_hex_color(hex: &str) -> Option<Rgba<u8>> {
    if !hex.is_ascii() {
        return None;
    }
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let red = parse_hex_nibble(chars.next()?)?;
            let green = parse_hex_nibble(chars.next()?)?;
            let blue = parse_hex_nibble(chars.next()?)?;
            Some(Rgba([red * 17, green * 17, blue * 17, 255]))
        }
        6 => {
            let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Rgba([red, green, blue, 255]))
        }
        _ => None,
    }
}

fn parse_hex_nibble(ch: char) -> Option<u8> {
    ch.to_digit(16).map(|value| value as u8)
}

fn color_to_hex(color: Rgba<u8>) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn terminal_color(color: Rgba<u8>) -> Color {
    Color::Rgb {
        r: color[0],
        g: color[1],
        b: color[2],
    }
}

fn truncate_to_cols(text: &str, cols: u16) -> String {
    text.chars().take(cols as usize).collect()
}

fn save_png(path: &Path, image: &image::RgbaImage) -> Result<()> {
    ensure_output_path(path)?;
    image
        .save(path)
        .with_context(|| format!("failed to write PNG output {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canvas::Point, terminal::TerminalMetrics, theme::ThemeMode};

    fn canvas() -> DrawingCanvas {
        DrawingCanvas::blank(
            TerminalMetrics::from_dimensions(10, 5, 100, 50),
            ThemeMode::Dark,
        )
    }

    fn state() -> AppState {
        AppState::new(Rgba([255, 255, 255, 255]))
    }

    fn layout() -> TerminalLayout {
        TerminalLayout::from_display_dimensions(10, 7, 100, 70, 1.0)
    }

    #[test]
    fn planned_shortcuts_update_canvas() {
        let mut canvas = canvas();
        let mut state = state();
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert_eq!(state.tool, DrawingTool::Rectangle);
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert_eq!(state.tool, DrawingTool::Ellipse);
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert_eq!(state.tool, DrawingTool::Freehand);
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(state.input_mode);
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(!state.input_mode);
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
            &mut canvas,
            &mut state
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut canvas,
            &mut state
        ));
    }

    #[test]
    fn mouse_events_create_completed_stroke() {
        let mut canvas = canvas();
        let mut state = state();
        let layout = layout();
        let mut mouse_mapper = MouseMapper {
            mode: MouseCoordinateMode::Cell,
        };
        assert!(handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut canvas,
            &mut state,
            layout,
            &mut mouse_mapper,
        ));
        assert!(handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 8,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut canvas,
            &mut state,
            layout,
            &mut mouse_mapper,
        ));
        assert!(handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 8,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut canvas,
            &mut state,
            layout,
            &mut mouse_mapper,
        ));
        assert!(canvas.undo());
        let image = canvas.render();
        assert_eq!(*image.get_pixel(50, 10), image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn mouse_mapper_uses_pixel_mode_for_large_coordinates() {
        let canvas = canvas();
        let layout = layout();
        let mut mapper = MouseMapper {
            mode: MouseCoordinateMode::Cell,
        };
        let target = mapper.target_for_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 50,
                row: 25,
                modifiers: KeyModifiers::NONE,
            },
            layout,
            &canvas,
        );
        assert_eq!(target, MouseTarget::Canvas(Point::new(0.5, 0.5)));
        assert_eq!(mapper.mode, MouseCoordinateMode::Pixel);
    }

    #[test]
    fn direct_canvas_points_are_available_for_tests() {
        let mut canvas = canvas();
        canvas.begin_stroke(Point::new(0.0, 0.0));
        canvas.finish_current();
        assert!(canvas.undo());
    }

    #[test]
    fn color_input_accepts_names_and_hex_values() {
        assert_eq!(parse_color("red"), Some(Rgba([255, 0, 0, 255])));
        assert_eq!(parse_color("BLUE"), Some(Rgba([30, 100, 255, 255])));
        assert_eq!(parse_color("#0f0"), Some(Rgba([0, 255, 0, 255])));
        assert_eq!(parse_color("#112233"), Some(Rgba([17, 34, 51, 255])));
        assert_eq!(parse_color("not-a-color"), None);
    }

    #[test]
    fn color_prompt_applies_valid_color_and_keeps_invalid_input_open() {
        let mut canvas = canvas();
        let mut state = state();
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        for ch in "bad".chars() {
            assert!(!handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &mut canvas,
                &mut state
            ));
        }
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(state.input_mode);
        assert_eq!(state.color, Rgba([255, 255, 255, 255]));
        assert!(state.message.contains("Unknown color"));

        state.color_buffer.clear();
        for ch in "#123456".chars() {
            assert!(!handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &mut canvas,
                &mut state
            ));
        }
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut canvas,
            &mut state
        ));
        assert!(!state.input_mode);
        assert_eq!(state.color, Rgba([18, 52, 86, 255]));
    }

    #[test]
    fn status_palette_click_changes_color_without_drawing() {
        let mut canvas = canvas();
        let mut state = state();
        let layout = TerminalLayout::from_display_dimensions(120, 7, 1200, 70, 1.0);
        let red_column = status_prefix(&state).chars().count() as u16
            + palette_swatch_width(PALETTE[0])
            + palette_swatch_width(PALETTE[1]);
        let mut mouse_mapper = MouseMapper {
            mode: MouseCoordinateMode::Cell,
        };

        assert!(handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: red_column,
                row: layout.status_row.unwrap(),
                modifiers: KeyModifiers::NONE,
            },
            &mut canvas,
            &mut state,
            layout,
            &mut mouse_mapper,
        ));

        assert_eq!(state.color, Rgba([255, 0, 0, 255]));
        assert!(!canvas.undo());
    }

    #[test]
    fn pixel_click_near_status_top_still_hits_palette() {
        let mut canvas = canvas();
        let mut state = state();
        let layout = TerminalLayout::from_display_dimensions(120, 7, 1200, 70, 1.0);
        let red_column = status_prefix(&state).chars().count() as u16
            + palette_swatch_width(PALETTE[0])
            + palette_swatch_width(PALETTE[1]);
        let pixel_column = red_column * 10 + 1;
        let status_top_pixel =
            (f32::from(layout.status_row.unwrap()) * layout.cell_height_px).floor() as u16;
        let mut mouse_mapper = MouseMapper {
            mode: MouseCoordinateMode::Pixel,
        };

        assert!(handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: pixel_column,
                row: status_top_pixel - 1,
                modifiers: KeyModifiers::NONE,
            },
            &mut canvas,
            &mut state,
            layout,
            &mut mouse_mapper,
        ));

        assert_eq!(state.color, Rgba([255, 0, 0, 255]));
        assert!(!canvas.undo());
    }
}
