use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Draw directly in Kitty-compatible terminals and save to PNG"
)]
pub struct Args {
    /// Optional image to draw on top of.
    pub input_image: Option<PathBuf>,

    /// Save output PNG path. Defaults to kitdraw1.png, kitdraw2.png, etc. or <input-stem>-kitdraw.png.
    #[arg(short = 'o', long = "output", value_parser = parse_png_output_path)]
    pub output: Option<PathBuf>,

    /// Drawing polarity for terminal contrast.
    #[arg(long = "theme", value_enum, default_value_t = ThemeArg::Auto)]
    pub theme: ThemeArg,

    /// Fallback terminal cell pixel size when terminal pixel dimensions are unavailable.
    #[arg(long = "cell-px", default_value = "10x20", value_parser = parse_cell_pixels)]
    pub cell_px: CellPixels,

    /// Canvas resolution relative to terminal pixel size. 0.5 uses half width and height.
    #[arg(long = "resolution-scale", default_value_t = 0.5, value_parser = parse_resolution_scale)]
    pub resolution_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ThemeArg {
    Auto,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPixels {
    pub width: u16,
    pub height: u16,
}

pub fn parse_cell_pixels(value: &str) -> std::result::Result<CellPixels, String> {
    let Some((width, height)) = value.split_once('x').or_else(|| value.split_once('X')) else {
        return Err("cell size must be WIDTHxHEIGHT".to_string());
    };
    let width = width
        .parse::<u16>()
        .map_err(|_| format!("invalid cell pixel width: {width}"))?;
    let height = height
        .parse::<u16>()
        .map_err(|_| format!("invalid cell pixel height: {height}"))?;
    if width == 0 || height == 0 {
        return Err("cell pixel dimensions must be greater than zero".to_string());
    }
    Ok(CellPixels { width, height })
}

pub fn parse_resolution_scale(value: &str) -> std::result::Result<f32, String> {
    let scale = value
        .parse::<f32>()
        .map_err(|_| format!("invalid resolution scale: {value}"))?;
    if !scale.is_finite() || !(0.1..=1.0).contains(&scale) {
        return Err("resolution scale must be between 0.1 and 1.0".to_string());
    }
    Ok(scale)
}

pub fn parse_png_output_path(value: &str) -> std::result::Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if has_png_extension(&path) {
        Ok(path)
    } else {
        Err("output file must have a .png extension".to_string())
    }
}

pub fn default_output_path(input_image: Option<&Path>) -> PathBuf {
    let Some(input) = input_image else {
        return next_available_numbered_path(Path::new("."), "kitdraw");
    };

    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("image");
    let file_name = format!("{stem}-kitdraw.png");
    input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}

fn next_available_numbered_path(directory: &Path, stem: &str) -> PathBuf {
    for idx in 1.. {
        let file_name = format!("{stem}{idx}.png");
        let candidate = directory.join(file_name);
        if !candidate.exists() {
            return if directory.as_os_str().is_empty() || directory == Path::new(".") {
                candidate
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or(candidate)
            } else {
                candidate
            };
        }
    }
    unreachable!("unbounded numbered output search should always return")
}

pub fn ensure_output_path(path: &Path) -> Result<()> {
    if !has_png_extension(path) {
        return Err(anyhow!("output file must have a .png extension"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if !parent.exists() {
                return Err(anyhow!(
                    "output parent directory does not exist: {}",
                    parent.display()
                ));
            }
            if !parent.is_dir() {
                return Err(anyhow!(
                    "output parent path is not a directory: {}",
                    parent.display()
                ));
            }
        }
    }
    Ok(())
}

fn has_png_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cell_pixels() {
        assert_eq!(
            parse_cell_pixels("12x24").unwrap(),
            CellPixels {
                width: 12,
                height: 24
            }
        );
        assert_eq!(
            parse_cell_pixels("8X16").unwrap(),
            CellPixels {
                width: 8,
                height: 16
            }
        );
        assert!(parse_cell_pixels("12").is_err());
        assert!(parse_cell_pixels("0x20").is_err());
    }

    #[test]
    fn parses_output_png_path() {
        assert_eq!(
            parse_png_output_path("out.png").unwrap(),
            PathBuf::from("out.png")
        );
        assert_eq!(
            parse_png_output_path("OUT.PNG").unwrap(),
            PathBuf::from("OUT.PNG")
        );
        assert!(parse_png_output_path("out.jpg").is_err());
        assert!(parse_png_output_path("out").is_err());
    }

    #[test]
    fn derives_default_output_path() {
        assert_eq!(
            default_output_path(Some(Path::new("photo.jpg"))),
            PathBuf::from("photo-kitdraw.png")
        );
        assert_eq!(
            default_output_path(Some(Path::new("images/photo.jpg"))),
            PathBuf::from("images/photo-kitdraw.png")
        );
    }

    #[test]
    fn derives_next_numbered_blank_output_path() {
        let dir = std::env::temp_dir().join(format!(
            "kitdraw_output_path_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = next_available_numbered_path(&dir, "kitdraw");
        assert_eq!(first, dir.join("kitdraw1.png"));
        std::fs::write(&first, []).unwrap();
        let second = next_available_numbered_path(&dir, "kitdraw");
        assert_eq!(second, dir.join("kitdraw2.png"));
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cli_accepts_planned_args() {
        let args = Args::try_parse_from([
            "kitdraw",
            "input.png",
            "-o",
            "out.png",
            "--theme",
            "light",
            "--cell-px",
            "12x24",
            "--resolution-scale",
            "0.25",
        ])
        .unwrap();
        assert_eq!(args.input_image.as_deref(), Some(Path::new("input.png")));
        assert_eq!(args.output.as_deref(), Some(Path::new("out.png")));
        assert_eq!(args.theme, ThemeArg::Light);
        assert_eq!(
            args.cell_px,
            CellPixels {
                width: 12,
                height: 24
            }
        );
        assert_eq!(args.resolution_scale, 0.25);
    }

    #[test]
    fn parses_resolution_scale() {
        assert_eq!(parse_resolution_scale("0.5").unwrap(), 0.5);
        assert_eq!(parse_resolution_scale("1").unwrap(), 1.0);
        assert_eq!(parse_resolution_scale("0.25").unwrap(), 0.25);
        assert!(parse_resolution_scale("0").is_err());
        assert!(parse_resolution_scale("1.5").is_err());
        assert!(parse_resolution_scale("nan").is_err());
    }
}
