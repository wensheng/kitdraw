use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    Command,
    cursor::{Hide, MoveTo, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, size, window_size,
    },
};

use crate::args::CellPixels;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalMetrics {
    pub cols: u16,
    pub rows: u16,
    pub display_width_px: u32,
    pub display_height_px: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub cell_width_px: f32,
    pub cell_height_px: f32,
}

impl TerminalMetrics {
    pub fn current(fallback: CellPixels, resolution_scale: f32) -> Self {
        let (cols, rows) = size().unwrap_or((80, 24));
        Self::from_cells(cols, rows, fallback, resolution_scale)
    }

    pub fn from_cells(cols: u16, rows: u16, fallback: CellPixels, resolution_scale: f32) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let window = window_size().ok();
        let display_width_px = window
            .as_ref()
            .filter(|window| window.width > 0 && window.height > 0)
            .map(|window| u32::from(window.width))
            .unwrap_or_else(|| u32::from(cols) * u32::from(fallback.width));
        let display_height_px = window
            .as_ref()
            .filter(|window| window.width > 0 && window.height > 0)
            .map(|window| u32::from(window.height))
            .unwrap_or_else(|| u32::from(rows) * u32::from(fallback.height));
        Self::from_display_dimensions(
            cols,
            rows,
            display_width_px.max(1),
            display_height_px.max(1),
            resolution_scale,
        )
    }

    #[cfg(test)]
    pub fn from_dimensions(cols: u16, rows: u16, width_px: u32, height_px: u32) -> Self {
        Self::from_display_dimensions(cols, rows, width_px, height_px, 1.0)
    }

    pub fn from_display_dimensions(
        cols: u16,
        rows: u16,
        display_width_px: u32,
        display_height_px: u32,
        resolution_scale: f32,
    ) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let display_width_px = display_width_px.max(1);
        let display_height_px = display_height_px.max(1);
        let resolution_scale = if resolution_scale.is_finite() {
            resolution_scale.clamp(0.1, 1.0)
        } else {
            0.5
        };
        let width_px = ((display_width_px as f32) * resolution_scale)
            .round()
            .max(1.0) as u32;
        let height_px = ((display_height_px as f32) * resolution_scale)
            .round()
            .max(1.0) as u32;
        Self {
            cols,
            rows,
            display_width_px,
            display_height_px,
            width_px,
            height_px,
            cell_width_px: width_px as f32 / f32::from(cols),
            cell_height_px: height_px as f32 / f32::from(rows),
        }
    }

    pub fn brush_radius_px(self) -> f32 {
        (self.cell_width_px.min(self.cell_height_px) * 0.175).max(0.75)
    }
}

pub struct TerminalSession;

impl TerminalSession {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableSgrPixelMouse,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        ) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            Show,
            DisableSgrPixelMouse,
            DisableMouseCapture,
            LeaveAlternateScreen,
            MoveTo(0, 0)
        );
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableSgrPixelMouse;

impl Command for EnableSgrPixelMouse {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1016h")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableSgrPixelMouse;

impl Command for DisableSgrPixelMouse {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1016l")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_scale_from_dimensions() {
        let metrics = TerminalMetrics::from_dimensions(80, 24, 800, 480);
        assert_eq!(metrics.cols, 80);
        assert_eq!(metrics.rows, 24);
        assert_eq!(metrics.display_width_px, 800);
        assert_eq!(metrics.display_height_px, 480);
        assert_eq!(metrics.width_px, 800);
        assert_eq!(metrics.height_px, 480);
        assert_eq!(metrics.cell_width_px, 10.0);
        assert_eq!(metrics.cell_height_px, 20.0);
    }

    #[test]
    fn brush_radius_comes_from_cell_size() {
        let metrics = TerminalMetrics::from_dimensions(80, 24, 800, 480);
        assert_eq!(metrics.brush_radius_px(), 1.75);
    }

    #[test]
    fn resolution_scale_reduces_canvas_size() {
        let metrics = TerminalMetrics::from_display_dimensions(80, 24, 800, 480, 0.5);
        assert_eq!(metrics.display_width_px, 800);
        assert_eq!(metrics.display_height_px, 480);
        assert_eq!(metrics.width_px, 400);
        assert_eq!(metrics.height_px, 240);
        assert_eq!(metrics.cell_width_px, 5.0);
        assert_eq!(metrics.cell_height_px, 10.0);
    }
}
