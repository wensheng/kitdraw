mod app;
mod args;
mod canvas;
mod kitty;
mod terminal;
mod theme;

use anyhow::Result;
use clap::Parser;

use crate::{
    app::AppConfig,
    args::{Args, default_output_path},
    theme::resolve_theme,
};

fn main() -> Result<()> {
    let args = Args::parse();
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(args.input_image.as_deref()));
    app::run(AppConfig {
        input_image: args.input_image,
        output,
        theme: resolve_theme(args.theme),
        fallback_cell_px: args.cell_px,
        resolution_scale: args.resolution_scale,
    })
}
