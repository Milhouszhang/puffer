use dirs::home_dir;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use regex_lite::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use url::Url;

#[derive(Clone, Copy)]
struct MarkdownStyles {
    h1: Style,
    h2: Style,
    h3: Style,
    h4: Style,
    h5: Style,
    h6: Style,
    code: Style,
    emphasis: Style,
    strong: Style,
    strikethrough: Style,
    ordered_list_marker: Style,
    unordered_list_marker: Style,
    link: Style,
    blockquote: Style,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        Self {
            h1: Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            h2: Style::default().add_modifier(Modifier::BOLD),
            h3: Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            h4: Style::default().add_modifier(Modifier::ITALIC),
            h5: Style::default().add_modifier(Modifier::ITALIC),
            h6: Style::default().add_modifier(Modifier::ITALIC),
            code: Style::default().fg(Color::Cyan),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            ordered_list_marker: Style::default().fg(Color::Blue),
            unordered_list_marker: Style::default(),
            link: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            blockquote: Style::default().fg(Color::Green),
        }
    }
}

#[derive(Clone, Debug)]
struct IndentContext {
    prefix: Vec<Span<'static>>,
    marker: Option<Vec<Span<'static>>>,
    is_list: bool,
}

impl IndentContext {
    fn new(prefix: Vec<Span<'static>>, marker: Option<Vec<Span<'static>>>, is_list: bool) -> Self {
        Self {
            prefix,
            marker,
            is_list,
        }
    }
}

#[derive(Clone, Debug)]
struct LinkState {
    destination: String,
    show_destination: bool,
    local_target_display: Option<String>,
}

pub(crate) static COLON_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$").expect("valid regex"));
pub(crate) static HASH_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$").expect("valid regex"));

/// Renders markdown into styled text using the same event-driven structure Codex uses.
pub(crate) fn render_markdown_text(input: &str) -> Text<'static> {
    render_markdown_text_with_width(input, None)
}

/// Renders markdown into styled text with an optional wrap width.
pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    let cwd = std::env::current_dir().ok();
    render_markdown_text_with_width_and_cwd(input, width, cwd.as_deref())
}

/// Renders markdown into styled text with explicit wrap width and working directory context.
pub(crate) fn render_markdown_text_with_width_and_cwd(
    input: &str,
    _width: Option<usize>,
    cwd: Option<&Path>,
) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, options);
    let mut writer = Writer::new(parser, cwd);
    writer.run();
    writer.text
}

struct Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    iter: I,
    text: Text<'static>,
    styles: MarkdownStyles,
    inline_styles: Vec<Style>,
    indent_stack: Vec<IndentContext>,
    list_indices: Vec<Option<u64>>,
    link: Option<LinkState>,
    needs_newline: bool,
    pending_marker_line: bool,
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_buffer: String,
    current_line_content: Option<Line<'static>>,
    current_initial_indent: Vec<Span<'static>>,
    current_line_style: Style,
    cwd: Option<PathBuf>,
}

impl<'a, I> Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn new(iter: I, cwd: Option<&Path>) -> Self {
        Self {
            iter,
            text: Text::default(),
            styles: MarkdownStyles::default(),
            inline_styles: Vec::new(),
            indent_stack: Vec::new(),
            list_indices: Vec::new(),
            link: None,
            needs_newline: false,
            pending_marker_line: false,
            in_code_block: false,
            code_block_lang: None,
            code_block_buffer: String::new(),
            current_line_content: None,
            current_initial_indent: Vec::new(),
            current_line_style: Style::default(),
            cwd: cwd.map(Path::to_path_buf),
        }
    }

    fn run(&mut self) {
        while let Some(event) = self.iter.next() {
            self.handle_event(event);
        }
        self.flush_current_line();
    }

    fn handle_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text),
            Event::Code(code) => self.code(code),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(text),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => {
                self.flush_current_line();
                if !self.text.lines.is_empty() {
                    self.push_blank_line();
                }
                self.push_line(Line::from("———"));
                self.needs_newline = true;
            }
            Event::Html(html) | Event::InlineHtml(html) => self.html(html),
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(_) => self.start_blockquote(),
            Tag::CodeBlock(kind) => {
                let (lang, indent) = match kind {
                    CodeBlockKind::Fenced(lang) => (extract_code_lang(&lang), None),
                    CodeBlockKind::Indented => (None, Some(Span::raw("    "))),
                };
                self.start_code_block(lang, indent);
            }
            Tag::List(start) => self.start_list(start),
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_inline_style(self.styles.emphasis),
            Tag::Strong => self.push_inline_style(self.styles.strong),
            Tag::Strikethrough => self.push_inline_style(self.styles.strikethrough),
            Tag::Link { dest_url, .. } => self.push_link(dest_url.to_string()),
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Image { .. }
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote(_) => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_code_block(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => {
                self.indent_stack.pop();
                self.pending_marker_line = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline_style(),
            TagEnd::Link => self.pop_link(),
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Image
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.push_line(Line::default());
        self.needs_newline = false;
    }

    fn end_paragraph(&mut self) {
        self.needs_newline = true;
        self.pending_marker_line = false;
    }

    fn start_heading(&mut self, level: HeadingLevel) {
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        let style = match level {
            HeadingLevel::H1 => self.styles.h1,
            HeadingLevel::H2 => self.styles.h2,
            HeadingLevel::H3 => self.styles.h3,
            HeadingLevel::H4 => self.styles.h4,
            HeadingLevel::H5 => self.styles.h5,
            HeadingLevel::H6 => self.styles.h6,
        };
        self.push_line(Line::from(vec![Span::styled(
            format!("{} ", "#".repeat(level as usize)),
            style,
        )]));
        self.push_inline_style(style);
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        self.pop_inline_style();
        self.needs_newline = true;
    }

    fn start_blockquote(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.indent_stack
            .push(IndentContext::new(vec![Span::raw("> ")], None, false));
    }

    fn end_blockquote(&mut self) {
        self.indent_stack.pop();
        self.needs_newline = true;
    }

    fn start_list(&mut self, index: Option<u64>) {
        if self.list_indices.is_empty() && self.needs_newline {
            self.push_line(Line::default());
        }
        self.list_indices.push(index);
    }

    fn end_list(&mut self) {
        self.list_indices.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        self.pending_marker_line = true;
        let depth = self.list_indices.len();
        let is_ordered = self
            .list_indices
            .last()
            .map(Option::is_some)
            .unwrap_or(false);
        let width = depth * 4 - 3;
        let marker = if let Some(last_index) = self.list_indices.last_mut() {
            match last_index {
                None => Some(vec![Span::styled(
                    " ".repeat(width.saturating_sub(1)) + "- ",
                    self.styles.unordered_list_marker,
                )]),
                Some(index) => {
                    *index += 1;
                    Some(vec![Span::styled(
                        format!("{:width$}. ", *index - 1),
                        self.styles.ordered_list_marker,
                    )])
                }
            }
        } else {
            None
        };
        let indent_prefix = if depth == 0 {
            Vec::new()
        } else {
            let indent_len = if is_ordered { width + 2 } else { width + 1 };
            vec![Span::raw(" ".repeat(indent_len))]
        };
        self.indent_stack
            .push(IndentContext::new(indent_prefix, marker, true));
        self.needs_newline = false;
    }

    fn start_code_block(&mut self, lang: Option<String>, indent: Option<Span<'static>>) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;
        self.code_block_lang = lang;
        self.code_block_buffer.clear();
        self.indent_stack.push(IndentContext::new(
            vec![indent.unwrap_or_default()],
            None,
            false,
        ));
        self.needs_newline = true;
    }

    fn end_code_block(&mut self) {
        let lang = self.code_block_lang.take();
        let code = std::mem::take(&mut self.code_block_buffer);
        let rendered = if lang.as_deref().is_some_and(|value| {
            value.eq_ignore_ascii_case("diff") || value.eq_ignore_ascii_case("patch")
        }) {
            diff_code_to_lines(&code)
        } else {
            plain_code_to_lines(&code, self.styles.code)
        };
        for line in rendered {
            self.push_line(line);
        }
        self.needs_newline = true;
        self.in_code_block = false;
        self.indent_stack.pop();
    }

    fn text(&mut self, text: CowStr<'a>) {
        if self.suppressing_local_link_label() {
            return;
        }
        if self.in_code_block {
            self.code_block_buffer.push_str(&text);
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        for (index, line) in text.split('\n').enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if index > 0 {
                self.push_line(Line::default());
            }
            if !line.is_empty() {
                self.push_span(Span::styled(
                    line.to_string(),
                    self.inline_styles.last().copied().unwrap_or_default(),
                ));
            }
        }
    }

    fn code(&mut self, code: CowStr<'a>) {
        if self.suppressing_local_link_label() {
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        self.push_span(Span::styled(code.into_string(), self.styles.code));
    }

    fn html(&mut self, html: CowStr<'a>) {
        if self.suppressing_local_link_label() {
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        for (index, line) in html.split('\n').enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if index > 0 {
                self.push_line(Line::default());
            }
            if !line.is_empty() {
                self.push_span(Span::styled(
                    line.to_string(),
                    self.inline_styles.last().copied().unwrap_or_default(),
                ));
            }
        }
        self.needs_newline = true;
    }

    fn soft_break(&mut self) {
        self.push_line(Line::default());
        self.needs_newline = false;
    }

    fn hard_break(&mut self) {
        self.push_line(Line::default());
        self.needs_newline = false;
    }

    fn push_inline_style(&mut self, style: Style) {
        let current = self.inline_styles.last().copied().unwrap_or_default();
        self.inline_styles.push(current.patch(style));
    }

    fn pop_inline_style(&mut self) {
        self.inline_styles.pop();
    }

    fn pop_link(&mut self) {
        if let Some(link) = self.link.take() {
            if link.show_destination {
                self.push_span(Span::raw(" ("));
                self.push_span(Span::styled(link.destination, self.styles.link));
                self.push_span(Span::raw(")"));
            } else if let Some(local_target_display) = link.local_target_display {
                self.push_span(Span::styled(local_target_display, self.styles.code));
            }
        }
    }

    fn push_link(&mut self, destination: String) {
        let is_local = is_local_path_like_link(&destination);
        self.link = Some(LinkState {
            show_destination: !is_local,
            local_target_display: if is_local {
                render_local_link_target(&destination, self.cwd.as_deref())
            } else {
                None
            },
            destination,
        });
    }

    fn suppressing_local_link_label(&self) -> bool {
        self.link
            .as_ref()
            .and_then(|link| link.local_target_display.as_ref())
            .is_some()
    }

    fn flush_current_line(&mut self) {
        if let Some(mut line) = self.current_line_content.take() {
            let mut spans = self.current_initial_indent.clone();
            spans.append(&mut line.spans);
            self.text
                .lines
                .push(Line::from(spans).style(self.current_line_style));
            self.current_initial_indent.clear();
            self.current_line_style = Style::default();
        }
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.flush_current_line();
        let blockquote_active = self
            .indent_stack
            .iter()
            .any(|ctx| ctx.prefix.iter().any(|span| span.content.contains('>')));
        let pending_marker_line = self.pending_marker_line;
        self.current_initial_indent = self.prefix_spans(pending_marker_line);
        self.current_line_style = if blockquote_active {
            self.styles.blockquote
        } else {
            line.style
        };
        self.current_line_content = Some(line);
        self.pending_marker_line = false;
    }

    fn push_span(&mut self, span: Span<'static>) {
        if let Some(line) = self.current_line_content.as_mut() {
            line.spans.push(span);
        } else {
            self.push_line(Line::from(vec![span]));
        }
    }

    fn push_blank_line(&mut self) {
        self.flush_current_line();
        if self.indent_stack.iter().all(|ctx| ctx.is_list) {
            self.text.lines.push(Line::default());
        } else {
            self.push_line(Line::default());
            self.flush_current_line();
        }
    }

    fn prefix_spans(&self, pending_marker_line: bool) -> Vec<Span<'static>> {
        let mut prefix = Vec::new();
        let last_marker_index = if pending_marker_line {
            self.indent_stack
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, context)| context.marker.as_ref().map(|_| index))
        } else {
            None
        };
        let last_list_index = self.indent_stack.iter().rposition(|ctx| ctx.is_list);

        for (index, context) in self.indent_stack.iter().enumerate() {
            if pending_marker_line {
                if Some(index) == last_marker_index {
                    if let Some(marker) = &context.marker {
                        prefix.extend(marker.iter().cloned());
                        continue;
                    }
                }
                if context.is_list
                    && last_marker_index.is_some_and(|marker_index| marker_index > index)
                {
                    continue;
                }
            } else if context.is_list && Some(index) != last_list_index {
                continue;
            }
            prefix.extend(context.prefix.iter().cloned());
        }

        prefix
    }
}

fn is_local_path_like_link(destination: &str) -> bool {
    destination.starts_with("file://")
        || destination.starts_with('/')
        || destination.starts_with("~/")
        || destination.starts_with("./")
        || destination.starts_with("../")
        || matches!(
            destination.as_bytes(),
            [drive, b':', separator, ..]
                if drive.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
        )
}

fn render_local_link_target(destination: &str, cwd: Option<&Path>) -> Option<String> {
    if !is_local_path_like_link(destination) {
        return None;
    }
    let (path_text, suffix) = parse_local_link_target(destination)?;
    let mut rendered = display_local_link_path(&path_text, cwd);
    if let Some(suffix) = suffix {
        rendered.push_str(&suffix);
    }
    Some(rendered)
}

fn parse_local_link_target(destination: &str) -> Option<(String, Option<String>)> {
    if destination.starts_with("file://") {
        let url = Url::parse(destination).ok()?;
        let mut path_text = url.to_file_path().ok()?.to_string_lossy().to_string();
        path_text = normalize_local_link_path_text(&path_text);
        let suffix = url
            .fragment()
            .filter(|fragment| HASH_LOCATION_SUFFIX_RE.is_match(fragment))
            .map(|fragment| format!("#{fragment}"));
        return Some((path_text, suffix));
    }

    let mut path_text = destination;
    let mut suffix = None;
    if let Some((candidate, fragment)) = destination.rsplit_once('#') {
        if HASH_LOCATION_SUFFIX_RE.is_match(fragment) {
            path_text = candidate;
            suffix = Some(format!("#{fragment}"));
        }
    }
    if suffix.is_none() {
        if let Some(found) = COLON_LOCATION_SUFFIX_RE.find(path_text) {
            if found.end() == path_text.len() {
                suffix = Some(found.as_str().to_string());
                path_text = &path_text[..found.start()];
            }
        }
    }

    Some((expand_local_link_path(path_text), suffix))
}

fn expand_local_link_path(path_text: &str) -> String {
    if let Some(rest) = path_text.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return normalize_local_link_path_text(&home.join(rest).to_string_lossy());
        }
    }
    normalize_local_link_path_text(path_text)
}

fn normalize_local_link_path_text(path_text: &str) -> String {
    if let Some(rest) = path_text.strip_prefix("\\\\") {
        format!("//{}", rest.replace('\\', "/").trim_start_matches('/'))
    } else {
        path_text.replace('\\', "/")
    }
}

fn display_local_link_path(path_text: &str, cwd: Option<&Path>) -> String {
    if let Some(cwd) = cwd {
        let cwd_text = normalize_local_link_path_text(&cwd.to_string_lossy());
        let cwd_prefix = cwd_text.trim_end_matches('/');
        if let Some(stripped) = path_text.strip_prefix(cwd_prefix) {
            let stripped = stripped.trim_start_matches('/');
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    path_text.to_string()
}

fn extract_code_lang(lang: &str) -> Option<String> {
    lang.split([',', ' ', '\t'])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn plain_code_to_lines(code: &str, style: Style) -> Vec<Line<'static>> {
    let mut lines = code
        .lines()
        .map(|line| Line::from(vec![Span::styled(line.to_string(), style)]))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn diff_code_to_lines(code: &str) -> Vec<Line<'static>> {
    let meta = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let added = Style::default().fg(Color::Green);
    let removed = Style::default().fg(Color::Red);
    let context = Style::default().fg(Color::DarkGray);

    let mut lines = code
        .lines()
        .map(|line| {
            let style = if line.starts_with("@@")
                || line.starts_with("diff ")
                || line.starts_with("index ")
                || line.starts_with("+++")
                || line.starts_with("---")
            {
                meta
            } else if line.starts_with('+') {
                added
            } else if line.starts_with('-') {
                removed
            } else {
                context
            };
            let mut rendered = Line::from(line.to_string());
            rendered.style = style;
            rendered
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_lines(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn empty() {
        assert_eq!(render_markdown_text(""), Text::default());
    }

    #[test]
    fn paragraph_single() {
        assert_eq!(
            text_lines(&render_markdown_text("Hello, world!")),
            vec!["Hello, world!"]
        );
    }

    #[test]
    fn headings() {
        let text = render_markdown_text("# Heading 1\n## Heading 2\n");
        assert_eq!(text_lines(&text), vec!["# Heading 1", "", "## Heading 2"]);
    }

    #[test]
    fn blockquote_single() {
        let text = render_markdown_text("> Blockquote");
        assert_eq!(text_lines(&text), vec!["> Blockquote"]);
    }

    #[test]
    fn lists_render_without_marker_only_rows() {
        let text = render_markdown_text("- List item 1\n- List item 2\n1. List item 3\n");
        assert_eq!(
            text_lines(&text),
            vec!["- List item 1", "- List item 2", "", "1. List item 3"]
        );
    }

    #[test]
    fn nested_lists_render_with_indent() {
        let text = render_markdown_text("- outer\n  - inner\n");
        assert_eq!(text_lines(&text), vec!["- outer", "    - inner"]);
    }

    #[test]
    fn links_append_destination() {
        let text = render_markdown_text("[docs](https://example.com/docs)");
        assert_eq!(text_lines(&text), vec!["docs (https://example.com/docs)"]);
    }

    #[test]
    fn code_diff_blocks_render_with_styles() {
        let text = render_markdown_text("```diff\n@@ hdr\n-old\n+new\n```\n");
        assert_eq!(text_lines(&text), vec!["@@ hdr", "-old", "+new"]);
        assert_ne!(text.lines[0].style, text.lines[1].style);
        assert_ne!(text.lines[1].style, text.lines[2].style);
    }

    #[test]
    fn block_spacing_exactly_one_blank_line() {
        // Every block-level transition should produce exactly 1 blank line.
        let text = render_markdown_text(
            "## Heading\n\nParagraph one.\n\nParagraph two.\n\n## Another\n\n- item\n",
        );
        assert_eq!(
            text_lines(&text),
            vec![
                "## Heading",
                "",               // 1 blank line after heading
                "Paragraph one.",
                "",               // 1 blank line between paragraphs
                "Paragraph two.",
                "",               // 1 blank line before heading
                "## Another",
                "",               // 1 blank line before list
                "- item",
            ]
        );
    }
}
