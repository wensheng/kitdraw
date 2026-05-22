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
};

use crate::{
    args::{CellPixels, ensure_output_path},
    canvas::{BaseSource, DrawingCanvas},
    kitty,
    terminal::{TerminalMetrics, TerminalSession},
    theme::ThemeMode,
};

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct AppConfig {
    pub input_image: Option<PathBuf>,
    pub output: PathBuf,
    pub theme: ThemeMode,
    pub fallback_cell_px: CellPixels,
    pub resolution_scale: f32,
}

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
    let metrics = TerminalMetrics::current(config.fallback_cell_px, config.resolution_scale);
    let mut canvas = match source {
        BaseSource::Blank => DrawingCanvas::blank(metrics, config.theme),
        BaseSource::Image(image) => {
            DrawingCanvas::new(metrics, BaseSource::Image(image), config.theme)
        }
    };
    let final_image = run_event_loop(
        &mut canvas,
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
    fallback_cell_px: CellPixels,
    resolution_scale: f32,
) -> Result<image::RgbaImage> {
    let mut stdout = io::stdout().lock();
    render_to_terminal(&mut stdout, canvas)?;
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
                        if handle_key(key, canvas) {
                            return Ok(canvas.render());
                        }
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        if handle_mouse(mouse, canvas, &mut mouse_mapper) {
                            dirty = true;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        canvas.resize(TerminalMetrics::from_cells(
                            cols,
                            rows,
                            fallback_cell_px,
                            resolution_scale,
                        ));
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
            render_to_terminal(&mut stdout, canvas)?;
            last_render = Instant::now();
            dirty = false;
        }
    }
}

fn handle_key(key: KeyEvent, canvas: &mut DrawingCanvas) -> bool {
    match key.code {
        KeyCode::Esc => true,
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char('z') => {
            canvas.undo();
            false
        }
        KeyCode::Char('c') => {
            canvas.clear();
            false
        }
        _ => false,
    }
}

fn handle_mouse(
    mouse: MouseEvent,
    canvas: &mut DrawingCanvas,
    mouse_mapper: &mut MouseMapper,
) -> bool {
    let point = mouse_mapper.point_for_mouse(mouse, canvas);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            canvas.begin_stroke(point);
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            canvas.extend_stroke(point);
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            canvas.extend_stroke(point);
            canvas.finish_stroke();
            true
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

    fn point_for_mouse(
        &mut self,
        mouse: MouseEvent,
        canvas: &DrawingCanvas,
    ) -> crate::canvas::Point {
        let metrics = canvas.metrics();
        if mouse.column >= metrics.cols || mouse.row >= metrics.rows {
            self.mode = MouseCoordinateMode::Pixel;
        }
        match self.mode {
            MouseCoordinateMode::Pixel => canvas.point_for_mouse_pixel(mouse.column, mouse.row),
            MouseCoordinateMode::Cell => canvas.point_for_mouse_cell(mouse.column, mouse.row),
        }
    }
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

fn render_to_terminal<W: Write>(writer: &mut W, canvas: &DrawingCanvas) -> Result<()> {
    let image = canvas.render();
    let metrics = canvas.metrics();
    queue!(writer, MoveTo(0, 0))?;
    kitty::write_frame(writer, &image, metrics.cols, metrics.rows, true)?;
    Ok(())
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
    use crate::{canvas::Point, theme::ThemeMode};

    fn canvas() -> DrawingCanvas {
        DrawingCanvas::blank(
            TerminalMetrics::from_dimensions(10, 5, 100, 50),
            ThemeMode::Dark,
        )
    }

    #[test]
    fn planned_shortcuts_update_canvas() {
        let mut canvas = canvas();
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
            &mut canvas
        ));
        assert!(!handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &mut canvas
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            &mut canvas
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut canvas
        ));
        assert!(handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut canvas
        ));
    }

    #[test]
    fn mouse_events_create_completed_stroke() {
        let mut canvas = canvas();
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
            &mut mouse_mapper,
        ));
        assert!(canvas.undo());
        let image = canvas.render();
        assert_eq!(*image.get_pixel(50, 10), image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn mouse_mapper_uses_pixel_mode_for_large_coordinates() {
        let canvas = canvas();
        let mut mapper = MouseMapper {
            mode: MouseCoordinateMode::Cell,
        };
        let point = mapper.point_for_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 50,
                row: 25,
                modifiers: KeyModifiers::NONE,
            },
            &canvas,
        );
        assert_eq!(point, Point::new(0.5, 0.5));
        assert_eq!(mapper.mode, MouseCoordinateMode::Pixel);
    }

    #[test]
    fn direct_canvas_points_are_available_for_tests() {
        let mut canvas = canvas();
        canvas.begin_stroke(Point::new(0.0, 0.0));
        canvas.finish_stroke();
        assert!(canvas.undo());
    }
}
