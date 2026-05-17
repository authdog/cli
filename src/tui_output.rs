//! Styled, scroll-aware status text for the Ratatui shell (slash-command output).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Clone, Copy)]
pub struct OutputPalette {
    #[allow(dead_code)]
    pub fg: Color,
    pub muted: Color,
    pub sep: Color,
    pub accent: Color,
    /// Strong success accent (e.g. post-login ✔ glyph).
    pub success: Color,
    pub ok: Color,
    pub err: Color,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DocBlock {
    Neutral,
    /// Pretty-printed JSON body after a `── … (…) ──` REST section banner.
    ApiJsonBody,
    AccessTokenClaims,
}

/// Section titles immediately followed by JSON (`/v1/userinfo`, `/v1/tenants`, etc.).
const JSON_SECTION_LINE_PREFIXES: &[&str] = &[
    "── Identity (",
    "── Tenants (",
    "── Organizations (",
    "── Projects (",
];

fn line_opens_api_json_section(line: &str) -> bool {
    let t = line.trim_start();
    JSON_SECTION_LINE_PREFIXES
        .iter()
        .any(|prefix| t.starts_with(prefix))
}

/// Estimate wrapped row count when using [`ratatui::widgets::Paragraph`] +
/// [`ratatui::widgets::Wrap`] `{ trim: true }`.
pub fn wrapped_row_count(lines: &[Line<'_>], width_cols: u16) -> usize {
    let w = width_cols.max(1) as usize;
    lines
        .iter()
        .map(|ln| {
            let cw = ln.width();
            usize::max(1, cw.saturating_add(w - 1) / w)
        })
        .sum()
}

pub fn styled_status_lines(
    text: &str,
    palette: OutputPalette,
    is_err_block: bool,
) -> Vec<Line<'static>> {
    let fg_base = Style::default().fg(if is_err_block {
        palette.err
    } else {
        palette.ok
    });

    let mut block = DocBlock::Neutral;
    let mut out: Vec<Line<'static>> = Vec::new();

    for line in text.lines() {
        let sep_access = line
            .trim_start()
            .starts_with("── Access token payload (decoded locally · signature NOT verified");

        if line_opens_api_json_section(line) {
            block = DocBlock::ApiJsonBody;
            out.push(Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(palette.sep).italic(),
            )]));
            continue;
        }

        if sep_access {
            block = DocBlock::AccessTokenClaims;
            out.push(Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(palette.sep).italic(),
            )]));
            continue;
        }

        if line.starts_with("credentials file:") {
            out.push(Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(palette.muted),
            )]));
            continue;
        }

        if block == DocBlock::ApiJsonBody && line.trim().is_empty() {
            block = DocBlock::Neutral;
            out.push(Line::default());
            continue;
        }

        match block {
            DocBlock::ApiJsonBody => out.push(highlight_json_line(line, palette)),
            DocBlock::AccessTokenClaims => {
                let tail_t = line.trim_start();

                let err_decode = tail_t.starts_with("(could not decode access token locally)")
                    || (line.starts_with("    ") && line.contains("could not decode"));

                if err_decode {
                    out.push(Line::from(vec![Span::styled(
                        line.to_string(),
                        Style::default().fg(palette.err),
                    )]));
                    continue;
                }

                let line_out = claims_or_neutral_body_line(line, tail_t, palette, fg_base);
                out.push(line_out);
            }
            DocBlock::Neutral => {
                out.push(neutral_success_line_maybe_check(line, palette, fg_base));
            }
        }
    }

    out
}

const HEAVY_CHECK: char = '\u{2714}';

fn neutral_success_line_maybe_check(
    line: &str,
    palette: OutputPalette,
    fg_base: Style,
) -> Line<'static> {
    if line.starts_with(HEAVY_CHECK) {
        let after_glyph = &line[HEAVY_CHECK.len_utf8()..];
        let after_trim = after_glyph.trim_start();
        let spacer = after_glyph.len().saturating_sub(after_trim.len());
        let space_str: String = " ".repeat(spacer.min(16));
        return Line::from(vec![
            Span::styled(
                format!("{HEAVY_CHECK}{space_str}"),
                Style::default().fg(palette.success).bold(),
            ),
            Span::styled(after_trim.to_string(), fg_base),
        ]);
    }

    Line::from(vec![Span::styled(line.to_string(), fg_base)])
}

fn claims_or_neutral_body_line(
    full: &str,
    trimmed: &str,
    palette: OutputPalette,
    base: Style,
) -> Line<'static> {
    let indent = &full[..full.len().saturating_sub(trimmed.len())];

    let first = trimmed.chars().next();
    let looks_like_json_piece = matches!(first, Some('{' | '}' | '[' | ']' | '"' | '\''))
        || trimmed.starts_with("null")
        || trimmed.starts_with("true")
        || trimmed.starts_with("false")
        || matches!(first, Some(c) if c.is_ascii_digit() || c == '-');

    if looks_like_json_piece {
        return prepend_indent(indent, highlight_json_line(trimmed, palette), palette);
    }

    highlight_maybe_claim_line(full, trimmed, indent, palette, base)
}

fn prepend_indent(indent: &str, body: Line<'static>, palette: OutputPalette) -> Line<'static> {
    if indent.is_empty() {
        return body;
    }
    let mut spans = vec![Span::styled(
        indent.to_string(),
        Style::default().fg(palette.muted),
    )];
    spans.extend(body.spans);
    Line::from(spans)
}

/// JWT claim lines (`key: value` or `Other claims:`) with indentation preserved on the left.
fn highlight_maybe_claim_line(
    _full: &str,
    trimmed: &str,
    indent: &str,
    palette: OutputPalette,
    base: Style,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    if !indent.is_empty() {
        spans.push(Span::styled(
            indent.to_string(),
            Style::default().fg(palette.muted),
        ));
    }

    if trimmed.starts_with("Other claims:") {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));
        return Line::from(spans);
    }

    spans.extend(highlight_kv_fallback_spans(trimmed, palette, base));
    Line::from(spans)
}

fn highlight_kv_fallback_spans(t: &str, palette: OutputPalette, base: Style) -> Vec<Span<'static>> {
    let Some(pi) = t.find(':') else {
        return vec![Span::styled(t.to_string(), base)];
    };

    let after = pi + ':'.len_utf8();
    let spacer_end = t[after..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(j, _)| after + j)
        .unwrap_or(t.len());

    let key_frag = t.get(..after).unwrap_or(t);
    let spacer = t.get(after..spacer_end).unwrap_or("");
    let tail = t.get(spacer_end..).unwrap_or("");

    vec![
        Span::styled(
            key_frag.to_string(),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(spacer.to_string(), Style::default().fg(palette.muted)),
        Span::styled(tail.to_string(), base),
    ]
}

#[allow(clippy::too_many_lines)]
fn highlight_json_line(line: &str, palette: OutputPalette) -> Line<'static> {
    let string_val = Style::default().fg(Color::Rgb(255, 218, 160));
    let number_s = Style::default().fg(Color::Rgb(160, 220, 255));
    let key_style = Style::default().fg(palette.accent);
    let kw_style = Style::default().fg(Color::Rgb(199, 170, 255));
    let punct = Style::default().fg(Color::Rgb(130, 120, 150));
    let ws_style = Style::default().fg(palette.muted);

    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    let mut spans: Vec<Span<'static>> = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            spans.push(Span::styled(
                chars[start..i].iter().collect::<String>(),
                ws_style,
            ));
            continue;
        }

        match c {
            '{' | '}' | '[' | ']' | ',' | ':' => {
                spans.push(Span::styled(c.to_string(), punct));
                i += 1;
            }
            '"' => {
                let (after_close, is_key) = scan_json_string(&chars, i);
                let frag: String = chars[i..after_close].iter().collect();
                spans.push(Span::styled(
                    frag,
                    if is_key { key_style } else { string_val },
                ));
                i = after_close;
            }
            '-' | '0'..='9' => {
                let start = i;
                i += 1;
                while i < chars.len() && matches!(chars[i], '0'..='9' | '.' | 'e' | 'E' | '+' | '-')
                {
                    i += 1;
                }
                spans.push(Span::styled(
                    chars[start..i].iter().collect::<String>(),
                    number_s,
                ));
            }
            _ => {
                if let Some((kw_fragment, consumed)) = try_json_keyword(&chars, i) {
                    spans.push(Span::styled(kw_fragment, kw_style));
                    i += consumed;
                } else {
                    spans.push(Span::styled(c.to_string(), punct));
                    i += 1;
                }
            }
        }
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

fn try_json_keyword(chars: &[char], i: usize) -> Option<(String, usize)> {
    const KWS: [&str; 3] = ["true", "false", "null"];
    let tail: String = chars[i..].iter().collect();
    for kw in KWS {
        if tail.starts_with(kw) {
            let end = i + kw.chars().count();
            if chars.get(end).is_none_or(boundary_after_json_lit) {
                return Some((kw.to_string(), kw.chars().count()));
            }
        }
    }
    None
}

fn boundary_after_json_lit(c: &char) -> bool {
    !(c.is_alphanumeric() || *c == '_')
}

fn scan_json_string(chars: &[char], open_quote_idx: usize) -> (usize, bool) {
    let mut i = open_quote_idx + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i = (i + 2).min(chars.len());
            }
            '"' => {
                let after_quote = i + 1;
                let mut j = after_quote;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let is_key = chars.get(j) == Some(&':');
                return (after_quote, is_key);
            }
            _ => i += 1,
        }
    }
    (chars.len(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal() -> OutputPalette {
        OutputPalette {
            fg: Color::White,
            muted: Color::Gray,
            sep: Color::Blue,
            accent: Color::Magenta,
            success: Color::Green,
            ok: Color::White,
            err: Color::Red,
        }
    }

    #[test]
    fn identity_separator_switches_highlighting() {
        let s = "── Identity (https://example/v1/userinfo) ──\n{\"x\":1}\n";
        let lines = styled_status_lines(s, pal(), false);
        assert!(lines.len() >= 2);
        assert!(
            lines[1].spans.iter().any(|sp| sp.content.contains('\"')),
            "{lines:?}",
        );
    }

    #[test]
    fn tenants_separator_switches_highlighting() {
        let s = "── Tenants (https://example/v1/tenants) ──\n{\"total\":1}\n";
        let lines = styled_status_lines(s, pal(), false);
        assert!(lines.len() >= 2);
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|sp| sp.content.contains("\"total\"")),
            "{lines:?}",
        );
    }

    #[test]
    fn organizations_separator_switches_highlighting() {
        let s = concat!(
            "── Organizations (https://example/v1/organizations) ──\n",
            "{\"organizations\":[{\"name\":\"Acme\"}]}\n",
        );
        let lines = styled_status_lines(s, pal(), false);
        assert!(lines.len() >= 2);
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|sp| sp.content.contains("\"name\"")),
            "{lines:?}",
        );
    }

    #[test]
    fn projects_separator_switches_highlighting() {
        let s = concat!(
            "── Projects (https://example/v1/tenants/tid/projects) ──\n",
            "{\"projects\":[]}\n",
        );
        let lines = styled_status_lines(s, pal(), false);
        assert!(lines.len() >= 2);
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|sp| sp.content.contains("projects")),
            "{lines:?}",
        );
    }

    #[test]
    fn heavy_check_line_splits_green_then_body() {
        let lines = styled_status_lines("\u{2714} Signed in", pal(), false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() >= 2);
        assert!(lines[0].spans[0].content.contains('\u{2714}'));
        assert_eq!(lines[0].spans[1].content, "Signed in");
    }

    #[test]
    fn claim_colon_splits_key_value() {
        let input = concat!(
            "── Access token payload (decoded locally · signature NOT verified · reference only) ──\n",
            "  sub:  google:user\n\n",
            "credentials file: /tmp/x\n",
        );
        let styled = styled_status_lines(input, pal(), false);
        let row = styled
            .iter()
            .find(|l| l.spans.iter().any(|sp| sp.content.contains("google:user")))
            .expect("sub line");
        assert!(row.spans.iter().any(|sp| sp.content.contains("sub:")));
    }
}
