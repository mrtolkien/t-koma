/// SVG-based table-to-PNG renderer for Discord messages.
///
/// Builds an SVG from parsed markdown table rows and rasterizes it via resvg.
/// Styled with Discord dark theme colors for seamless inline display.
use std::fmt::Write;
use std::sync::LazyLock;

use resvg::tiny_skia;
use resvg::usvg;
use tracing::warn;

const FONT_SIZE: f32 = 14.0;
/// Approximate pixel width of one monospace character at [`FONT_SIZE`].
/// Slightly overestimated to prevent text overflow across column boundaries.
const CHAR_WIDTH: f32 = 9.0;
const ROW_HEIGHT: f32 = 28.0;
const CELL_PAD_X: f32 = 12.0;
const CORNER_RADIUS: f32 = 8.0;

// Discord dark-theme palette
const BG_COLOR: &str = "#2B2D31";
const HEADER_BG: &str = "#1E1F22";
const TEXT_COLOR: &str = "#DBDEE1";
const HEADER_TEXT: &str = "#F2F3F5";
const BORDER_COLOR: &str = "#3F4147";
const HEADER_BORDER: &str = "#4E5058";

static SVG_OPTIONS: LazyLock<usvg::Options> = LazyLock::new(|| {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    opt
});

/// Render a parsed markdown table to PNG bytes.
///
/// `raw_lines` are the original markdown table lines (header, separator, data).
/// The separator row is stripped automatically.
/// Returns `None` if rendering fails (missing fonts, empty table, etc.).
pub(super) fn render_table_png(raw_lines: &[String]) -> Option<Vec<u8>> {
    let rows: Vec<Vec<String>> = raw_lines
        .iter()
        .filter(|l| !is_separator_line(l))
        .map(|l| parse_cells(l))
        .collect();

    if rows.is_empty() {
        return None;
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return None;
    }

    // Column widths in characters (minimum 3 to avoid tiny columns)
    let mut col_chars = vec![3usize; col_count];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                col_chars[i] = col_chars[i].max(cell.chars().count());
            }
        }
    }

    let col_px: Vec<f32> = col_chars
        .iter()
        .map(|&n| n as f32 * CHAR_WIDTH + 2.0 * CELL_PAD_X)
        .collect();

    let total_w = col_px.iter().sum::<f32>().ceil();
    let total_h = (rows.len() as f32 * ROW_HEIGHT).ceil();

    let svg = build_svg(&rows, &col_px, col_count, total_w, total_h);

    match rasterize(&svg) {
        Ok(png) => Some(png),
        Err(e) => {
            warn!("Table image render failed: {e}");
            None
        }
    }
}

fn build_svg(rows: &[Vec<String>], col_px: &[f32], col_count: usize, w: f32, h: f32) -> String {
    let mut s = String::with_capacity(2048);

    // Root + background
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}">"#
    );
    let _ = write!(
        s,
        r#"<rect width="{w}" height="{h}" fill="{BG_COLOR}" rx="{CORNER_RADIUS}"/>"#,
    );

    // Clip path for rounded corners on the header
    let _ = write!(
        s,
        r#"<defs><clipPath id="c"><rect width="{w}" height="{h}" rx="{CORNER_RADIUS}"/></clipPath></defs>"#,
    );
    let _ = write!(
        s,
        r#"<rect width="{w}" height="{ROW_HEIGHT}" fill="{HEADER_BG}" clip-path="url(#c)"/>"#,
    );

    // Header separator line
    let _ = write!(
        s,
        r#"<line x1="0" y1="{ROW_HEIGHT}" x2="{w}" y2="{ROW_HEIGHT}" stroke="{HEADER_BORDER}" stroke-width="1"/>"#,
    );

    // Data row separators
    for i in 2..rows.len() {
        let y = i as f32 * ROW_HEIGHT;
        let _ = write!(
            s,
            r#"<line x1="0" y1="{y}" x2="{w}" y2="{y}" stroke="{BORDER_COLOR}" stroke-width="0.5"/>"#,
        );
    }

    // Column separators
    let mut x = 0.0;
    for &cw in col_px.iter().take(col_count - 1) {
        x += cw;
        let _ = write!(
            s,
            r#"<line x1="{x}" y1="0" x2="{x}" y2="{h}" stroke="{BORDER_COLOR}" stroke-width="0.5"/>"#,
        );
    }

    // Cell text
    for (row_idx, row) in rows.iter().enumerate() {
        let is_header = row_idx == 0;
        let fill = if is_header { HEADER_TEXT } else { TEXT_COLOR };
        let weight = if is_header { "bold" } else { "normal" };
        let baseline_y = row_idx as f32 * ROW_HEIGHT + ROW_HEIGHT * 0.68;

        let mut col_x = 0.0_f32;
        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx >= col_count {
                break;
            }
            let tx = col_x + CELL_PAD_X;
            let escaped = xml_escape(cell);
            let _ = write!(
                s,
                r#"<text x="{tx}" y="{baseline_y}" font-family="monospace, 'DejaVu Sans Mono', 'Liberation Mono'" font-size="{FONT_SIZE}" fill="{fill}" font-weight="{weight}">{escaped}</text>"#,
            );
            col_x += col_px[col_idx];
        }
    }

    s.push_str("</svg>");
    s
}

fn rasterize(svg_str: &str) -> Result<Vec<u8>, String> {
    let tree = usvg::Tree::from_data(svg_str.as_bytes(), &SVG_OPTIONS)
        .map_err(|e| format!("SVG parse: {e}"))?;

    let size = tree.size().to_int_size();
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("pixmap allocation failed")?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    pixmap.encode_png().map_err(|e| format!("PNG encode: {e}"))
}

fn parse_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !(trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner
        .split('|')
        .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_returns_none() {
        assert!(render_table_png(&[]).is_none());
    }

    #[test]
    fn separator_only_returns_none() {
        let lines = vec!["|---|---|".to_string()];
        assert!(render_table_png(&lines).is_none());
    }

    #[test]
    fn simple_table_renders_png() {
        let lines = vec![
            "| Name | Value |".to_string(),
            "|------|-------|".to_string(),
            "| A    | 1     |".to_string(),
            "| B    | 2     |".to_string(),
        ];
        let png = render_table_png(&lines);
        assert!(
            png.is_some(),
            "PNG rendering failed — system fonts may be missing"
        );
        let bytes = png.unwrap();
        // PNG magic bytes
        assert_eq!(&bytes[..4], b"\x89PNG");
    }

    #[test]
    #[ignore = "writes preview file to /tmp — run manually"]
    fn write_preview_png() {
        let lines = vec![
            "| Kind | Identifier / Location | Summary |".to_string(),
            "|------|-----------------------|---------|".to_string(),
            "| Reference topic | 3d-printers (topic ID 42a525e0) | Enclosed 3D printers for home use |".to_string(),
            "| Web fetch | https://www.3dsourced.com/printers/ | Top three enclosed 3D printers |".to_string(),
            "| Memory capture | ping test | Operator sent a ping test. |".to_string(),
        ];
        let png = render_table_png(&lines).expect("render failed");
        std::fs::write("/tmp/t-koma-table-preview.png", &png).expect("write failed");
        eprintln!("Preview written to /tmp/t-koma-table-preview.png");
    }

    #[test]
    fn xml_special_chars_escaped() {
        assert_eq!(xml_escape("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }

    #[test]
    fn parse_cells_strips_padding() {
        let cells = parse_cells("| Hello | World |");
        assert_eq!(cells, vec!["Hello", "World"]);
    }

    #[test]
    fn separator_detection() {
        assert!(is_separator_line("|---|---|"));
        assert!(is_separator_line("| --- | :---: |"));
        assert!(!is_separator_line("| A | B |"));
    }
}
