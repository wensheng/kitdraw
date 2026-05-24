# kitdraw

**Sketch, annotate, and save PNGs or SVGs without leaving your terminal.**

(Part of kit* series of graphic terminal apps:
[kitim](https://github.com/wensheng/kitim)
[kitmd](https://github.com/wensheng/kitmd)
[kitpdf](https://github.com/wensheng/kitpdf)
[kitdraw](https://github.com/wensheng/kitdraw)
[kitDOOM](https://github.com/wensheng/kitdoom))

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
- **Save clean PNG or SVG output automatically** with undo, clear, color controls, shape tools, arrows, text, highlighter, redaction, contrast-aware default ink, and adjustable render resolution for speed.

---

## Usage

```bash
kitdraw
kitdraw screenshot.png
kitdraw screenshot.png --resolution-scale 0.25 -o notes.png
kitdraw screenshot.png -o notes.svg
kitdraw screenshot.png --export-size canvas -o terminal-sized.png
```

---

## How It Works

```text
terminal size -> scaled RGBA canvas -> mouse annotations -> zlib Kitty frames -> autosaved PNG/SVG
```

The fast path is intentionally simple: keep a committed image for completed drawing elements, preview only the active annotation, compress RGBA frames with Kitty's graphics protocol, and write the final composited canvas on exit. Image annotations export at the original image size by default; use `--export-size canvas` to keep the terminal canvas dimensions.

---

## Controls

| Action | Control |
| --- | --- |
| Draw active tool | Left mouse drag |
| Freehand tool | `f` |
| Rectangle tool | `r` |
| Ellipse tool | `e` |
| Arrow tool | `a` |
| Text tool | `t`, then click, type, and press `Enter` |
| Highlighter tool | `h` |
| Redaction tool | `x` |
| Change stroke/text size | `[` / `]` |
| Change color | Click a status-bar swatch, or press `c` and enter a color name/hex value |
| Undo | `z` |
| Clear drawing layer | `C` |
| Save and quit | `q`, `Esc`, or `Ctrl-C` |
