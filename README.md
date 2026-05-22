# kitdraw

**Sketch, annotate, and save PNGs without leaving your terminal.**

(kitdraw runs on terminals that supports the Kitty graphics protocol:
[**Ghostty**](https://ghostty.org/),
[**Kitty**](https://sw.kovidgoyal.net/kitty/),
[**WezTerm**](https://wezterm.net/),
[**cmux**](https://github.com/manaflow-ai/cmux))

## Install

    cargo install kitdraw

---

## Why

- Stop jumping from terminal to Preview, markup tools, or browser tabs just to circle something in a screenshot.
- Turn quick visual notes into real PNG files immediately, with numbered saves that do not clobber your last sketch.
- Stay keyboard-light and mouse-direct: no tool palettes, no modes to babysit, no canvas gymnastics.

---

<!-- ## Show, Don't Tell

![kitdraw demo placeholder](./assets/demo.gif)
-->

## Key Capabilities

- **Draw directly on your terminal canvas** with Kitty/Ghostty graphics and smooth mouse-driven strokes.
- **Annotate existing images in place** by loading a PNG/JPEG/etc. and drawing right over it.
- **Save clean PNG output automatically** with undo, clear, contrast-aware ink, and adjustable render resolution for speed.

---

## Usage

```bash
kitdraw
kitdraw screenshot.png
kitdraw screenshot.png --resolution-scale 0.25 -o notes.png
```

---

## How It Works

```text
terminal size -> scaled RGBA canvas -> mouse strokes -> zlib Kitty frames -> autosaved PNG
```

The fast path is intentionally simple: keep a committed image for completed strokes, preview only the active stroke, compress RGBA frames with Kitty's graphics protocol, and write the final composited canvas as a PNG on exit.

---

## Controls

| Action | Control |
| --- | --- |
| Draw | Left mouse drag |
| Undo | `z` |
| Clear drawing layer | `c` |
| Save and quit | `q`, `Esc`, or `Ctrl-C` |

