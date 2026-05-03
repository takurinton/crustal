use std::collections::HashMap;

use lsp_types::{
    Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkedString, Position, Range, Url,
};

#[derive(Debug, Clone)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Default)]
pub struct DocumentStore {
    documents: HashMap<Url, String>,
}

impl DocumentStore {
    pub fn did_open(&mut self, uri: Url, text: String) -> Vec<Diagnostic> {
        self.documents.insert(uri.clone(), text);
        self.diagnostics(&uri)
    }

    pub fn did_change(&mut self, uri: Url, text: String) -> Vec<Diagnostic> {
        self.documents.insert(uri.clone(), text);
        self.diagnostics(&uri)
    }

    pub fn did_close(&mut self, uri: &Url) -> Vec<Diagnostic> {
        self.documents.remove(uri);
        Vec::new()
    }

    pub fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        self.documents
            .get(uri)
            .and_then(|text| hover_at(text, position))
    }

    pub fn diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        self.documents
            .get(uri)
            .map(|text| analyze(text).diagnostics)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacroKind {
    Render,
    Css,
    GlobalCss,
}

#[derive(Debug, Clone)]
struct MacroInvocation {
    kind: MacroKind,
    name_range: ByteRange,
    body: ByteRange,
    whole: ByteRange,
    closed: bool,
}

#[derive(Debug, Clone)]
struct HoverItem {
    range: ByteRange,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }
}

struct LineIndex<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { text, line_starts }
    }

    fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        };
        let line_start = self.line_starts[line];
        let character = self.text[line_start..offset]
            .chars()
            .map(|ch| ch.len_utf16() as u32)
            .sum();
        Position {
            line: line as u32,
            character,
        }
    }

    fn range(&self, range: ByteRange) -> Range {
        Range {
            start: self.position(range.start),
            end: self.position(range.end),
        }
    }

    fn offset(&self, position: Position) -> Option<usize> {
        let line_start = *self.line_starts.get(position.line as usize)?;
        let line_end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        let mut utf16 = 0;
        for (idx, ch) in self.text[line_start..line_end].char_indices() {
            if utf16 == position.character {
                return Some(line_start + idx);
            }
            utf16 += ch.len_utf16() as u32;
            if utf16 > position.character {
                return Some(line_start + idx);
            }
        }
        Some(line_end)
    }
}

pub fn analyze(text: &str) -> Analysis {
    let line_index = LineIndex::new(text);
    let invocations = scan_macros(text);
    let mut diagnostics = Vec::new();
    for invocation in &invocations {
        match invocation.kind {
            MacroKind::Render => {
                diagnose_render(text, &line_index, invocation, &mut diagnostics, None)
            }
            MacroKind::Css | MacroKind::GlobalCss => {
                diagnose_css(text, &line_index, invocation, &mut diagnostics, None)
            }
        }
    }
    Analysis { diagnostics }
}

pub fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let line_index = LineIndex::new(text);
    let offset = line_index.offset(position)?;
    let invocations = scan_macros(text);
    for invocation in &invocations {
        if !invocation.whole.contains(offset) {
            continue;
        }
        let mut hovers = Vec::new();
        match invocation.kind {
            MacroKind::Render => {
                let mut diagnostics = Vec::new();
                diagnose_render(
                    text,
                    &line_index,
                    invocation,
                    &mut diagnostics,
                    Some(&mut hovers),
                );
            }
            MacroKind::Css | MacroKind::GlobalCss => {
                let mut diagnostics = Vec::new();
                diagnose_css(
                    text,
                    &line_index,
                    invocation,
                    &mut diagnostics,
                    Some(&mut hovers),
                );
            }
        }
        return hovers
            .into_iter()
            .find(|item| item.range.contains(offset))
            .map(|item| Hover {
                contents: HoverContents::Scalar(MarkedString::String(item.message)),
                range: Some(line_index.range(item.range)),
            });
    }
    None
}

fn scan_macros(text: &str) -> Vec<MacroInvocation> {
    let bytes = text.as_bytes();
    let mut invocations = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(next) = skip_rust_trivia(text, i) {
            i = next;
            continue;
        }
        if !is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        let ident_start = i;
        i += 1;
        while i < bytes.len() && is_ident_continue(bytes[i]) {
            i += 1;
        }
        let ident = &text[ident_start..i];
        let kind = match ident {
            "render" => MacroKind::Render,
            "css" => MacroKind::Css,
            "global_css" => MacroKind::GlobalCss,
            _ => continue,
        };
        let mut j = skip_ws(text, i);
        if bytes.get(j) != Some(&b'!') {
            continue;
        }
        j = skip_ws(text, j + 1);
        let Some(&open) = bytes.get(j) else {
            continue;
        };
        if !matches!(open, b'{' | b'(' | b'[') {
            continue;
        }
        let close = matching_close(open);
        let (end, closed) = scan_balanced(text, j, open, close);
        invocations.push(MacroInvocation {
            kind,
            name_range: ByteRange::new(ident_start, i),
            body: ByteRange::new(j + 1, end.saturating_sub(usize::from(closed))),
            whole: ByteRange::new(ident_start, end),
            closed,
        });
        i = end.max(j + 1);
    }
    invocations
}

fn diagnose_render(
    text: &str,
    line_index: &LineIndex<'_>,
    invocation: &MacroInvocation,
    diagnostics: &mut Vec<Diagnostic>,
    mut hovers: Option<&mut Vec<HoverItem>>,
) {
    if !invocation.closed {
        push_diag(
            diagnostics,
            line_index,
            ByteRange::new(invocation.whole.end.saturating_sub(1), invocation.whole.end),
            "unclosed render! macro delimiter",
        );
    }

    let mut stack: Vec<(String, ByteRange, bool)> = Vec::new();
    let mut i = invocation.body.start;
    while i < invocation.body.end {
        let byte = text.as_bytes()[i];
        if byte == b'<' {
            if text.as_bytes().get(i + 1) == Some(&b'/') {
                let close_start = i;
                i += 2;
                let name_start = i;
                while i < invocation.body.end && is_tag_name_continue(text.as_bytes()[i]) {
                    i += 1;
                }
                let name = text[name_start..i].to_string();
                let name_range = ByteRange::new(name_start, i);
                while i < invocation.body.end && text.as_bytes()[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= invocation.body.end || text.as_bytes()[i] != b'>' {
                    push_diag(
                        diagnostics,
                        line_index,
                        ByteRange::new(close_start, i.min(invocation.body.end)),
                        "missing '>' in closing tag",
                    );
                    continue;
                }
                i += 1;
                if let Some((open_name, _, matched)) = stack.last_mut() {
                    if *open_name == name {
                        *matched = true;
                        stack.pop();
                        push_hover(
                            &mut hovers,
                            name_range,
                            format!("Crustal render element `<{name}>`.\n\nMatched with its opening tag."),
                        );
                    } else {
                        push_diag(
                            diagnostics,
                            line_index,
                            name_range,
                            format!("mismatched closing tag: expected `</{}>`", open_name),
                        );
                        push_hover(
                            &mut hovers,
                            name_range,
                            format!("Crustal render element `<{name}>`.\n\nCurrently mismatched; expected `</{}>`.", open_name),
                        );
                    }
                } else {
                    push_diag(
                        diagnostics,
                        line_index,
                        name_range,
                        "unexpected closing tag",
                    );
                    push_hover(
                        &mut hovers,
                        name_range,
                        format!("Crustal render element `<{name}>`.\n\nNo matching opening tag is currently in scope."),
                    );
                }
            } else {
                let open_start = i;
                i += 1;
                let name_start = i;
                while i < invocation.body.end && is_tag_name_continue(text.as_bytes()[i]) {
                    i += 1;
                }
                if name_start == i {
                    push_diag(
                        diagnostics,
                        line_index,
                        ByteRange::new(open_start, (open_start + 1).min(invocation.body.end)),
                        "missing tag name",
                    );
                    continue;
                }
                let name = text[name_start..i].to_string();
                let name_range = ByteRange::new(name_start, i);
                let attr_start = i;
                let tag_end =
                    find_tag_end(text, i, invocation.body.end).unwrap_or(invocation.body.end);
                parse_attributes(
                    text,
                    line_index,
                    ByteRange::new(attr_start, tag_end),
                    diagnostics,
                    &mut hovers,
                );
                if tag_end >= invocation.body.end || text.as_bytes()[tag_end] != b'>' {
                    push_diag(
                        diagnostics,
                        line_index,
                        ByteRange::new(open_start, tag_end),
                        "missing '>' in opening tag",
                    );
                    i = tag_end;
                } else {
                    i = tag_end + 1;
                }
                stack.push((name.clone(), name_range, false));
                push_hover(
                    &mut hovers,
                    name_range,
                    format!("Crustal render element `<{name}>`.\n\nSSR emits an HTML tag; the client renderer creates a DOM element."),
                );
            }
        } else if byte == b'{' {
            let expr_start = i;
            let (expr_end, closed) = scan_balanced(text, i, b'{', b'}');
            let clamped_end = expr_end.min(invocation.body.end);
            if !closed || expr_end > invocation.body.end {
                push_diag(
                    diagnostics,
                    line_index,
                    ByteRange::new(expr_start, clamped_end),
                    "unbalanced braced expression in render!",
                );
                i = clamped_end.max(expr_start + 1);
            } else {
                push_hover(
                    &mut hovers,
                    ByteRange::new(expr_start, expr_end),
                    "Crustal render expression.\n\nSSR appends `ToString::to_string(&expr)`; the client renderer binds through `crustal_wasm::Bindable::bind`.".to_string(),
                );
                i = expr_end;
            }
        } else if let Some(next) = skip_rust_trivia(text, i) {
            i = next.min(invocation.body.end);
        } else {
            i += 1;
        }
    }

    for (name, range, matched) in stack {
        if !matched {
            push_diag(
                diagnostics,
                line_index,
                range,
                format!("missing closing tag for `<{name}>`"),
            );
        }
    }
}

fn parse_attributes(
    text: &str,
    line_index: &LineIndex<'_>,
    range: ByteRange,
    diagnostics: &mut Vec<Diagnostic>,
    hovers: &mut Option<&mut Vec<HoverItem>>,
) {
    let mut i = range.start;
    while i < range.end {
        i = skip_ws_until(text, i, range.end);
        if i >= range.end {
            break;
        }
        if text.as_bytes()[i] == b'/' {
            push_diag(
                diagnostics,
                line_index,
                ByteRange::new(i, (i + 1).min(range.end)),
                "self-closing render! tags are not supported",
            );
            i += 1;
            continue;
        }
        let key_start = i;
        while i < range.end && is_attr_key_continue(text.as_bytes()[i]) {
            i += 1;
        }
        if key_start == i {
            i += 1;
            continue;
        }
        let key_range = ByteRange::new(key_start, i);
        i = skip_ws_until(text, i, range.end);
        if i >= range.end || text.as_bytes()[i] != b'=' {
            push_diag(
                diagnostics,
                line_index,
                key_range,
                "attribute is missing `=`",
            );
            continue;
        }
        i += 1;
        i = skip_ws_until(text, i, range.end);
        if i >= range.end || text.as_bytes()[i] == b'/' {
            push_diag(
                diagnostics,
                line_index,
                ByteRange::new(key_range.start, i),
                "attribute is missing a value",
            );
            continue;
        }
        let value_start = i;
        let value_end = match text.as_bytes()[i] {
            b'"' | b'\'' => scan_string(text, i).unwrap_or(range.end),
            b'{' => scan_balanced(text, i, b'{', b'}').0.min(range.end),
            _ => {
                while i < range.end && !text.as_bytes()[i].is_ascii_whitespace() {
                    i += 1;
                }
                i
            }
        };
        i = value_end.max(value_start + 1);
        push_hover(
            hovers,
            key_range,
            "Crustal render attribute.\n\nSSR serializes the value with `ToString` into `key=\"value\"`; the client renderer calls `set_attribute` with the same string value.".to_string(),
        );
    }
}

fn diagnose_css(
    text: &str,
    line_index: &LineIndex<'_>,
    invocation: &MacroInvocation,
    diagnostics: &mut Vec<Diagnostic>,
    hovers: Option<&mut Vec<HoverItem>>,
) {
    if !invocation.closed {
        push_diag(
            diagnostics,
            line_index,
            ByteRange::new(invocation.whole.end.saturating_sub(1), invocation.whole.end),
            "unclosed CSS macro delimiter",
        );
    }
    let Some(css) = extract_css_text(text, invocation, line_index, diagnostics) else {
        return;
    };
    diagnose_css_text(
        &css.text,
        css.range_offset,
        invocation.kind,
        line_index,
        diagnostics,
    );

    if let Some(hovers) = hovers {
        let hash = fnv1a_hash(&css.text) & 0x00ff_ffff;
        match invocation.kind {
            MacroKind::Css => {
                let class_name = format!("css-{hash:06x}");
                let preview = generate_css_preview(&class_name, &css.text);
                hovers.push(HoverItem {
                    range: invocation.name_range,
                    message: format!(
                        "`css!` generates class `{class_name}`.\n\nCompiled selector preview:\n```css\n{preview}\n```"
                    ),
                });
            }
            MacroKind::GlobalCss => {
                let id = format!("gcss-{hash:06x}");
                hovers.push(HoverItem {
                    range: invocation.name_range,
                    message: format!("`global_css!` injects CSS with style id `{id}`."),
                });
            }
            MacroKind::Render => {}
        }
    }
}

struct ExtractedCss {
    text: String,
    range_offset: usize,
}

fn extract_css_text(
    text: &str,
    invocation: &MacroInvocation,
    line_index: &LineIndex<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ExtractedCss> {
    let body = &text[invocation.body.start..invocation.body.end];
    let trimmed_start = body.len() - body.trim_start().len();
    let trimmed_end = body.trim_end().len();
    let start = invocation.body.start + trimmed_start;
    let end = invocation.body.start + trimmed_end;
    if start >= end {
        return Some(ExtractedCss {
            text: String::new(),
            range_offset: start,
        });
    }
    let bytes = text.as_bytes();
    if matches!(bytes[start], b'"' | b'\'') {
        let Some(string_end) = scan_string(text, start) else {
            push_diag(
                diagnostics,
                line_index,
                ByteRange::new(start, end),
                "unclosed string literal in CSS macro",
            );
            return None;
        };
        if string_end > end {
            push_diag(
                diagnostics,
                line_index,
                ByteRange::new(start, end),
                "unclosed string literal in CSS macro",
            );
            return None;
        }
        let value = unescape_simple_string(&text[start + 1..string_end - 1]);
        Some(ExtractedCss {
            text: value,
            range_offset: start + 1,
        })
    } else {
        Some(ExtractedCss {
            text: body.trim().to_string(),
            range_offset: start,
        })
    }
}

fn diagnose_css_text(
    css: &str,
    range_offset: usize,
    kind: MacroKind,
    line_index: &LineIndex<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut stack = Vec::new();
    for (idx, ch) in css.char_indices() {
        match ch {
            '{' => {
                if kind == MacroKind::Css {
                    let selector_start =
                        css[..idx].rfind(['}', ';']).map(|pos| pos + 1).unwrap_or(0);
                    let selector = css[selector_start..idx].trim();
                    if !selector.is_empty()
                        && !selector.starts_with('@')
                        && !selector.starts_with('&')
                        && !selector.contains(':')
                    {
                        push_diag(
                            diagnostics,
                            line_index,
                            ByteRange::new(range_offset + selector_start, range_offset + idx),
                            "nested selector blocks in css! must start with `&`",
                        );
                    }
                }
                stack.push(idx);
            }
            '}' => {
                if stack.pop().is_none() {
                    push_diag(
                        diagnostics,
                        line_index,
                        ByteRange::new(range_offset + idx, range_offset + idx + 1),
                        "unbalanced CSS closing brace",
                    );
                }
            }
            _ => {}
        }
    }
    for idx in stack {
        push_diag(
            diagnostics,
            line_index,
            ByteRange::new(range_offset + idx, range_offset + idx + 1),
            "unbalanced CSS opening brace",
        );
    }

    let mut segment_start = 0;
    let mut depth = 0usize;
    for (idx, ch) in css.char_indices() {
        match ch {
            '{' => {
                check_declaration_segment(
                    css,
                    segment_start,
                    idx,
                    range_offset,
                    line_index,
                    diagnostics,
                );
                depth += 1;
                segment_start = idx + 1;
            }
            '}' => {
                check_declaration_segment(
                    css,
                    segment_start,
                    idx,
                    range_offset,
                    line_index,
                    diagnostics,
                );
                depth = depth.saturating_sub(1);
                segment_start = idx + 1;
            }
            ';' if depth <= 1 => segment_start = idx + 1,
            _ => {}
        }
    }
    check_declaration_segment(
        css,
        segment_start,
        css.len(),
        range_offset,
        line_index,
        diagnostics,
    );
}

fn check_declaration_segment(
    css: &str,
    start: usize,
    end: usize,
    range_offset: usize,
    line_index: &LineIndex<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let segment = css[start..end].trim();
    if segment.is_empty()
        || segment.starts_with('@')
        || segment.starts_with('&')
        || segment.contains('{')
    {
        return;
    }
    if segment.contains(':') && !segment.ends_with(';') {
        push_diag(
            diagnostics,
            line_index,
            ByteRange::new(range_offset + start, range_offset + end),
            "unterminated CSS declaration; expected `;` before the next rule or macro end",
        );
    }
}

fn generate_css_preview(class_name: &str, css: &str) -> String {
    let css = css.trim();
    if css.is_empty() {
        return format!(".{class_name} {{ }}");
    }
    let mut parts = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].trim();
        let after = &rest[open + 1..];
        let close = after.find('}').unwrap_or(after.len());
        let props = after[..close].trim();
        if selector.starts_with('&') {
            parts.push(format!(
                "{} {{ {} }}",
                selector.replace('&', &format!(".{class_name}")),
                props
            ));
        } else if selector.starts_with("@media") {
            let query = selector.trim_start_matches("@media").trim();
            parts.push(format!("@media {query} {{ .{class_name} {{ {props} }} }}"));
        }
        rest = if close < after.len() {
            &after[close + 1..]
        } else {
            ""
        };
    }
    let top = css
        .split(['&', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(';');
    if !top.is_empty() && top.contains(':') {
        parts.insert(0, format!(".{class_name} {{ {top}; }}"));
    }
    if parts.is_empty() {
        format!(".{class_name} {{ {css} }}")
    } else {
        parts.join("\n")
    }
}

fn push_diag(
    diagnostics: &mut Vec<Diagnostic>,
    line_index: &LineIndex<'_>,
    range: ByteRange,
    message: impl Into<String>,
) {
    diagnostics.push(Diagnostic {
        range: line_index.range(range),
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("crustal-lsp".to_string()),
        message: message.into(),
        related_information: None,
        tags: None,
        data: None,
    });
}

fn push_hover(hovers: &mut Option<&mut Vec<HoverItem>>, range: ByteRange, message: String) {
    if let Some(hovers) = hovers.as_deref_mut() {
        hovers.push(HoverItem { range, message });
    }
}

fn skip_ws(text: &str, mut i: usize) -> usize {
    while i < text.len() && text.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn skip_ws_until(text: &str, mut i: usize, end: usize) -> usize {
    while i < end && text.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn skip_rust_trivia(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    match bytes.get(i).copied()? {
        b'"' | b'\'' => scan_string(text, i),
        b'/' if bytes.get(i + 1) == Some(&b'/') => Some(
            text[i..]
                .find('\n')
                .map(|n| i + n + 1)
                .unwrap_or(text.len()),
        ),
        b'/' if bytes.get(i + 1) == Some(&b'*') => Some(
            text[i + 2..]
                .find("*/")
                .map(|n| i + n + 4)
                .unwrap_or(text.len()),
        ),
        _ => None,
    }
}

fn scan_string(text: &str, start: usize) -> Option<usize> {
    let quote = text.as_bytes()[start];
    let mut i = start + 1;
    while i < text.len() {
        match text.as_bytes()[i] {
            b'\\' => i += 2,
            b if b == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn scan_balanced(text: &str, start: usize, open: u8, close: u8) -> (usize, bool) {
    let mut depth = 0usize;
    let mut i = start;
    while i < text.len() {
        if let Some(next) = skip_rust_trivia(text, i) {
            i = next;
            continue;
        }
        let byte = text.as_bytes()[i];
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return (i + 1, true);
            }
        }
        i += 1;
    }
    (text.len(), false)
}

fn find_tag_end(text: &str, mut i: usize, end: usize) -> Option<usize> {
    while i < end {
        if let Some(next) = skip_rust_trivia(text, i) {
            i = next.min(end);
            continue;
        }
        if text.as_bytes()[i] == b'{' {
            i = scan_balanced(text, i, b'{', b'}').0.min(end);
            continue;
        }
        if text.as_bytes()[i] == b'>' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn matching_close(open: u8) -> u8 {
    match open {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        _ => open,
    }
}

fn fnv1a_hash(s: &str) -> u32 {
    let mut hash: u32 = 2166136261;
    for byte in s.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16777619);
    }
    hash
}

fn unescape_simple_string(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn is_tag_name_continue(byte: u8) -> bool {
    is_ident_continue(byte) || matches!(byte, b'-' | b':')
}

fn is_attr_key_continue(byte: u8) -> bool {
    is_tag_name_continue(byte) || byte == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(text: &str) -> Vec<String> {
        analyze(text)
            .diagnostics
            .into_iter()
            .map(|diag| diag.message)
            .collect()
    }

    fn hover_text(text: &str, needle: &str) -> String {
        let offset = text.find(needle).unwrap();
        let position = LineIndex::new(text).position(offset);
        let hover = hover_at(text, position).unwrap();
        match hover.contents {
            HoverContents::Scalar(MarkedString::String(value)) => value,
            _ => panic!("unexpected hover shape"),
        }
    }

    #[test]
    fn scans_multiple_macros_and_ignores_strings() {
        let text = r#"fn main() {
            let _ = "render! { <bad> }";
            let a = render! { <div>{ "{x}" }</div> };
            let b = css!("color: red;");
            global_css! { body { margin: 0; } }
        }"#;
        let invocations = scan_macros(text);
        assert_eq!(invocations.len(), 3);
    }

    #[test]
    fn scanner_handles_nested_rust_braces_and_string_delimiters() {
        let text = r#"render!({ <div>{ format!("}})") }</div> }) css! { color: red; }"#;
        let invocations = scan_macros(text);
        assert_eq!(invocations.len(), 2);
        assert!(invocations.iter().all(|inv| inv.closed));
    }

    #[test]
    fn render_valid_nested_tags() {
        assert!(messages("render! { <div><span>{name}</span></div> }").is_empty());
    }

    #[test]
    fn render_reports_mismatched_missing_and_unexpected_tags() {
        assert!(messages("render! { <div></span> }")
            .iter()
            .any(|msg| msg.contains("mismatched closing tag")));
        assert!(messages("render! { <div><span></div> }")
            .iter()
            .any(|msg| msg.contains("missing closing tag")));
        assert!(messages("render! { </div> }")
            .iter()
            .any(|msg| msg.contains("unexpected closing tag")));
    }

    #[test]
    fn render_reports_attribute_syntax_errors() {
        let msgs = messages("render! { <div class id=></div> }");
        assert!(msgs.iter().any(|msg| msg.contains("missing `=`")));
        assert!(msgs.iter().any(|msg| msg.contains("missing a value")));
    }

    #[test]
    fn render_reports_unbalanced_expression_and_macro_delimiter() {
        let msgs = messages("render! { <div>{ value </div>");
        assert!(msgs
            .iter()
            .any(|msg| msg.contains("unclosed render! macro")));
        assert!(msgs
            .iter()
            .any(|msg| msg.contains("unbalanced braced expression")));
    }

    #[test]
    fn css_accepts_string_token_nested_and_media_forms() {
        assert!(messages(r#"css!("color: red;")"#).is_empty());
        assert!(messages("css! { color: red; &:hover { color: blue; } }").is_empty());
        assert!(messages("css! { @media (max-width: 768px) { font-size: 12px; } }").is_empty());
    }

    #[test]
    fn css_reports_unbalanced_unterminated_and_invalid_nested_selector() {
        let msgs = messages("css! { color: red &:hover { color: blue; } h1 { color: black; }");
        assert!(msgs.iter().any(|msg| msg.contains("unclosed CSS macro")));
        assert!(msgs
            .iter()
            .any(|msg| msg.contains("unterminated CSS declaration")));
        assert!(msgs.iter().any(|msg| msg.contains("must start with `&`")));
    }

    #[test]
    fn css_hover_uses_fnv_hash_class_name() {
        let hover = hover_text(r#"let c = css!("color: red;");"#, "css");
        assert!(hover.contains("css-044b6e"));
        assert!(hover.contains(".css-044b6e"));
    }

    #[test]
    fn global_css_hover_uses_style_id() {
        let hover = hover_text(r#"global_css!("body { margin: 0; }");"#, "global_css");
        assert!(hover.contains("gcss-"));
    }

    #[test]
    fn render_hover_describes_tags_attributes_and_exprs() {
        assert!(
            hover_text(r#"render! { <div class="x">{name}</div> }"#, "div")
                .contains("Crustal render element")
        );
        assert!(
            hover_text(r#"render! { <div class="x">{name}</div> }"#, "class")
                .contains("set_attribute")
        );
        assert!(
            hover_text(r#"render! { <div class="x">{name}</div> }"#, "{name}")
                .contains("Bindable::bind")
        );
    }

    #[test]
    fn hover_returns_none_outside_supported_regions() {
        let position = Position {
            line: 0,
            character: 0,
        };
        assert!(hover_at("fn main() {}", position).is_none());
    }

    #[test]
    fn document_store_open_change_close_and_hover() {
        let uri = Url::parse("file:///tmp/app.rs").unwrap();
        let mut store = DocumentStore::default();
        let diagnostics = store.did_open(uri.clone(), "render! { <div></span> }".to_string());
        assert!(diagnostics
            .iter()
            .any(|diag| diag.message.contains("mismatched closing tag")));

        let clean = store.did_change(uri.clone(), "render! { <div></div> }".to_string());
        assert!(clean.is_empty());

        let hover = store.hover(
            &uri,
            Position {
                line: 0,
                character: 11,
            },
        );
        assert!(hover.is_some());

        let cleared = store.did_close(&uri);
        assert!(cleared.is_empty());
        assert!(store.hover(&uri, Position::default()).is_none());
    }
}
