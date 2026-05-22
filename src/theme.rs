use std::{
    env,
    io::{self, Read, Write},
    time::{Duration, Instant},
};

use image::Rgba;

use crate::args::ThemeArg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn background(self) -> Rgba<u8> {
        match self {
            Self::Dark => Rgba([0, 0, 0, 255]),
            Self::Light => Rgba([255, 255, 255, 255]),
        }
    }

    pub fn stroke(self) -> Rgba<u8> {
        match self {
            Self::Dark => Rgba([255, 255, 255, 255]),
            Self::Light => Rgba([0, 0, 0, 255]),
        }
    }
}

pub fn resolve_theme(theme: ThemeArg) -> ThemeMode {
    match theme {
        ThemeArg::Dark => ThemeMode::Dark,
        ThemeArg::Light => ThemeMode::Light,
        ThemeArg::Auto => query_terminal_background_mode()
            .or_else(theme_from_colorfgbg_env)
            .unwrap_or(ThemeMode::Dark),
    }
}

fn query_terminal_background_mode() -> Option<ThemeMode> {
    query_terminal_background_rgb(Duration::from_millis(120)).map(rgb_to_theme)
}

fn theme_from_colorfgbg_env() -> Option<ThemeMode> {
    colorfgbg_to_theme(&env::var("COLORFGBG").ok()?)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
fn query_terminal_background_rgb(timeout: Duration) -> Option<(u8, u8, u8)> {
    use crossterm::terminal::enable_raw_mode;
    use std::os::fd::AsRawFd;

    enable_raw_mode().ok()?;
    let _restore_raw = RawModeRestore;

    let mut stdout = io::stdout();
    stdout.write_all(b"\x1b]11;?\x1b\\").ok()?;
    stdout.flush().ok()?;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let original_flags = unsafe { fcntl_getfl(fd) }.ok()?;
    let _flags = FcntlFlagsRestore {
        fd,
        flags: original_flags,
    };
    unsafe { fcntl_setfl(fd, original_flags | O_NONBLOCK_CONST) }.ok()?;

    let mut reader = stdin.lock();
    let mut response = Vec::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut buffer = [0u8; 128];
        match reader.read(&mut buffer) {
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(bytes) => {
                response.extend_from_slice(&buffer[..bytes]);
                if let Some(rgb) = parse_osc_11_response(&response) {
                    return Some(rgb);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }

    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn query_terminal_background_rgb(_timeout: Duration) -> Option<(u8, u8, u8)> {
    None
}

pub fn colorfgbg_to_theme(value: &str) -> Option<ThemeMode> {
    let background = value
        .split(';')
        .next_back()
        .and_then(|part| part.parse::<u8>().ok())?;
    if is_bright_ansi_color(background) {
        Some(ThemeMode::Light)
    } else {
        Some(ThemeMode::Dark)
    }
}

fn is_bright_ansi_color(color: u8) -> bool {
    matches!(color, 7 | 9..=15)
}

fn rgb_to_theme((red, green, blue): (u8, u8, u8)) -> ThemeMode {
    let luminance =
        (0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue)) / 255.0;
    if luminance >= 0.5 {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

fn parse_osc_11_response(bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("]11;")? + 4;
    let rest = &text[start..];
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    parse_rgb_color(&rest[..end])
}

fn parse_rgb_color(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.strip_prefix("rgb:")?;
    let mut parts = value.split('/');
    let red = parse_rgb_component(parts.next()?)?;
    let green = parse_rgb_component(parts.next()?)?;
    let blue = parse_rgb_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((red, green, blue))
}

fn parse_rgb_component(value: &str) -> Option<u8> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4 {
        return None;
    }
    let parsed = u16::from_str_radix(value, 16).ok()?;
    let max = (1u32 << (value.len() * 4)) - 1;
    Some(((u32::from(parsed) * 255) / max) as u8)
}

struct RawModeRestore;

impl Drop for RawModeRestore {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
        {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
struct FcntlFlagsRestore {
    fd: i32,
    flags: i32,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
impl Drop for FcntlFlagsRestore {
    fn drop(&mut self) {
        let _ = unsafe { fcntl_setfl(self.fd, self.flags) };
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
const F_GETFL_CONST: i32 = 3;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
const F_SETFL_CONST: i32 = 4;
#[cfg(target_os = "macos")]
const O_NONBLOCK_CONST: i32 = 0x0004;
#[cfg(any(target_os = "linux", target_os = "android"))]
const O_NONBLOCK_CONST: i32 = 0o4000;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
unsafe fn fcntl_getfl(fd: i32) -> io::Result<i32> {
    let result = unsafe { fcntl(fd, F_GETFL_CONST) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
unsafe fn fcntl_setfl(fd: i32, flags: i32) -> io::Result<()> {
    let result = unsafe { fcntl(fd, F_SETFL_CONST, flags) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colorfgbg_background() {
        assert_eq!(colorfgbg_to_theme("15;0"), Some(ThemeMode::Dark));
        assert_eq!(colorfgbg_to_theme("0;15"), Some(ThemeMode::Light));
        assert_eq!(colorfgbg_to_theme("0;7"), Some(ThemeMode::Light));
        assert_eq!(colorfgbg_to_theme("0;4"), Some(ThemeMode::Dark));
        assert_eq!(colorfgbg_to_theme("bad"), None);
    }

    #[test]
    fn theme_colors_are_opposite() {
        assert_eq!(ThemeMode::Dark.background(), Rgba([0, 0, 0, 255]));
        assert_eq!(ThemeMode::Dark.stroke(), Rgba([255, 255, 255, 255]));
        assert_eq!(ThemeMode::Light.background(), Rgba([255, 255, 255, 255]));
        assert_eq!(ThemeMode::Light.stroke(), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn parses_osc_11_rgb_response() {
        assert_eq!(
            parse_osc_11_response(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
            Some((255, 255, 255))
        );
        assert_eq!(
            parse_osc_11_response(b"\x1b]11;rgb:0000/0000/0000\x07"),
            Some((0, 0, 0))
        );
        assert_eq!(rgb_to_theme((255, 255, 255)), ThemeMode::Light);
        assert_eq!(rgb_to_theme((0, 0, 0)), ThemeMode::Dark);
    }
}
