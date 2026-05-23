mod app;
mod args;
mod canvas;
mod export;
mod kitty;
mod terminal;
mod theme;

use anyhow::Result;
use clap::Parser;

use crate::{
    app::AppConfig,
    args::{Args, default_export_size, default_output_path, resolve_output_format},
    theme::resolve_theme,
};

fn main() -> Result<()> {
    let args = Args::parse();
    let output_format = resolve_output_format(args.format, args.output.as_deref())?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(args.input_image.as_deref(), output_format));
    let export_size = args
        .export_size
        .unwrap_or_else(|| default_export_size(args.input_image.as_deref()));
    app::run(AppConfig {
        input_image: args.input_image,
        output,
        output_format,
        export_size,
        theme: resolve_theme(args.theme),
        fallback_cell_px: args.cell_px,
        resolution_scale: args.resolution_scale,
    })
}
