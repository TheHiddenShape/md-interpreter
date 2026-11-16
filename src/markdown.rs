//! Markdown -> HTML interpreter, with no dependencies.
//!
//! Supported: headings, paragraphs, bold/italic/strikethrough, inline code,
//! code blocks (``` or ~~~), links, images, lists (nested, bulleted,
//! numbered, checkboxes), blockquotes, tables, horizontal rules.
//! Raw HTML is escaped (shown as-is, never executed).

use std::collections::HashMap;

/// Converts a Markdown document to HTML.
pub fn render(markdown: &str) -> String {
    let lines: Vec<String> = markdown
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| {
            let tabs = l.len() - l.trim_start_matches('\t').len();
            format!("{}{}", "    ".repeat(tabs), &l[tabs..])
        })
        .collect();
    blocks(&lines, &mut HashMap::new())
}

/// Escapes HTML special characters.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    s.chars().for_each(|c| push_escaped(&mut out, c));
    out
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        _ => out.push(c),
    }
}

fn safe_url(url: &str) -> String {
    let lower = url.trim_start().to_ascii_lowercase();
    if ["javascript:", "vbscript:", "data:"].iter().any(|p| lower.starts_with(p)) {
        "#".to_string()
    } else {
        escape(url)
    }
}

// ---------- Block level: headings, lists, tables, etc. ----------

type Ids = HashMap<String, u32>;

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn is_blank(l: &str) -> bool {
    l.trim().is_empty()
}

/// Opening of a code block: (character, fence length, language).
fn fence(l: &str) -> Option<(char, usize, &str)> {
    if indent_of(l) > 3 {
        return None;
    }
    let t = l.trim_start();
    let ch = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|&c| c == ch).count();
    let rest = t[n..].trim();
    if n < 3 || (ch == '`' && rest.contains('`')) {
        return None;
    }
    Some((ch, n, rest.split_whitespace().next().unwrap_or("")))
}

fn closes_fence(l: &str, ch: char, n: usize) -> bool {
    let t = l.trim();
    indent_of(l) <= 3 && t.len() >= n && t.chars().all(|c| c == ch)
}

/// Heading `# ...`: (level, text).
fn heading(l: &str) -> Option<(usize, &str)> {
    if indent_of(l) > 3 {
        return None;
    }
    let t = l.trim_start();
    let level = t.len() - t.trim_start_matches('#').len();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let text = rest.trim();
    // Closing #s (`## Title ##`) are removed, but not the one in "C#".
    let stripped = text.trim_end_matches('#');
    if stripped.is_empty() || stripped.ends_with(' ') {
        Some((level, stripped.trim_end()))
    } else {
        Some((level, text))
    }
}

fn is_hr(l: &str) -> bool {
    let t: Vec<char> = l.chars().filter(|c| *c != ' ').collect();
    indent_of(l) <= 3 && t.len() >= 3 && "-*_".contains(t[0]) && t.iter().all(|c| *c == t[0])
}

/// Blockquote line: returns its content without the `>`.
fn quote(l: &str) -> Option<&str> {
    if indent_of(l) > 3 {
        return None;
    }
    let rest = l.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

struct Item<'a> {
    indent: usize,
    /// Width of the marker and the spaces after it.
    width: usize,
    ordered: bool,
    start: u32,
    content: &'a str,
}

/// List item: `- x`, `* x`, `+ x`, `1. x`, `1) x`.
fn item(l: &str) -> Option<Item<'_>> {
    let indent = indent_of(l);
    let rest = &l[indent..];
    let b = rest.as_bytes();
    let (marker, ordered, start) = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        (1, false, 1)
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !matches!(b.get(digits), Some(b'.' | b')')) {
            return None;
        }
        (digits + 1, true, rest[..digits].parse().ok()?)
    };
    let after = &rest[marker..];
    let spaces = after.len() - after.trim_start_matches(' ').len();
    if spaces == 0 {
        return None;
    }
    let used = if spaces > 4 { 1 } else { spaces };
    Some(Item { indent, width: marker + used, ordered, start, content: &after[used..] })
}

fn starts_block(l: &str) -> bool {
    fence(l).is_some() || heading(l).is_some() || is_hr(l) || quote(l).is_some() || item(l).is_some()
}

/// Splits a table row into cells (`\|` is not a separator).
fn cells(l: &str) -> Vec<String> {
    let t = l.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = vec![String::new()];
    let mut escaped = false;
    for c in t.chars() {
        if c == '|' && !escaped {
            out.push(String::new());
        } else {
            out.last_mut().unwrap().push(c);
        }
        escaped = c == '\\' && !escaped;
    }
    out.iter().map(|c| c.trim().to_string()).collect()
}

/// Row `|---|:---:|`: returns the alignment of each column.
fn table_aligns(l: &str) -> Option<Vec<&'static str>> {
    if !l.contains('-') {
        return None;
    }
    cells(l)
        .iter()
        .map(|c| {
            let dashes = c.trim_start_matches(':').trim_end_matches(':');
            if dashes.is_empty() || !dashes.chars().all(|d| d == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => "center",
                (false, true) => "right",
                (true, false) => "left",
                _ => "",
            })
        })
        .collect()
}

/// Anchor id for a heading: "Café Ideas" -> `cafe-ideas`.
fn slug(text: &str, ids: &mut Ids) -> String {
    let mut s = String::new();
    for c in text.to_lowercase().chars() {
        let plain = match c {
            'à' | 'â' | 'ä' => "a",
            'ç' => "c",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'î' | 'ï' => "i",
            'ô' | 'ö' => "o",
            'ù' | 'û' | 'ü' => "u",
            'œ' => "oe",
            'æ' => "ae",
            _ => "",
        };
        if !plain.is_empty() {
            s.push_str(plain);
        } else if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
    }
    let mut s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        s.push_str("section");
    }
    let n = ids.entry(s.clone()).or_insert(0);
    *n += 1;
    if *n == 1 { s } else { format!("{s}-{n}") }
}

fn blocks(lines: &[String], ids: &mut Ids) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].as_str();

        if is_blank(line) {
            i += 1;
            continue;
        }

        // Code block
        if let Some((ch, n, lang)) = fence(line) {
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !closes_fence(&lines[i], ch, n) {
                code.push_str(&lines[i]);
                code.push('\n');
                i += 1;
            }
            i += 1; // closing line
            let class = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape(lang))
            };
            out.push(format!("<pre><code{class}>{}</code></pre>", escape(&code)));
            continue;
        }

        // Heading
        if let Some((level, text)) = heading(line) {
            out.push(format!("<h{level} id=\"{}\">{}</h{level}>", slug(text, ids), inline(text)));
            i += 1;
            continue;
        }

        // Horizontal rule
        if is_hr(line) {
            out.push("<hr>".to_string());
            i += 1;
            continue;
        }

        // Blockquote
        if quote(line).is_some() {
            let mut inner = Vec::new();
            while i < lines.len() && !is_blank(&lines[i]) {
                match quote(&lines[i]) {
                    Some(q) => inner.push(q.to_string()),
                    None if !starts_block(&lines[i]) => inner.push(lines[i].clone()),
                    None => break,
                }
                i += 1;
            }
            out.push(format!("<blockquote>\n{}\n</blockquote>", blocks(&inner, ids)));
            continue;
        }

        // List
        if let Some(first) = item(line) {
            let (base, ordered, start) = (first.indent, first.ordered, first.start);
            let same_list = |l: &str| item(l).is_some_and(|it| it.indent == base && it.ordered == ordered);
            let mut items: Vec<Vec<String>> = Vec::new();
            let mut loose = false;

            while i < lines.len() && same_list(&lines[i]) {
                let it = item(&lines[i]).unwrap();
                let content_indent = base + it.width;
                let belongs = |l: &str| {
                    indent_of(l) >= content_indent || (indent_of(l) > base && item(l).is_some())
                };
                let mut body = vec![it.content.to_string()];
                i += 1;

                while i < lines.len() {
                    let l = lines[i].as_str();
                    if is_blank(l) {
                        let mut j = i;
                        while j < lines.len() && is_blank(&lines[j]) {
                            j += 1;
                        }
                        if j < lines.len() && belongs(&lines[j]) {
                            body.push(String::new());
                            i = j;
                            continue;
                        }
                        if j < lines.len() && same_list(&lines[j]) {
                            loose = true;
                            i = j;
                        }
                        break;
                    }
                    if belongs(l) {
                        body.push(l[indent_of(l).min(content_indent)..].to_string());
                    } else if starts_block(l) {
                        break;
                    } else {
                        body.push(l.trim().to_string()); // unindented paragraph continuation
                    }
                    i += 1;
                }
                items.push(body);
            }

            let lis: Vec<String> = items
                .into_iter()
                .map(|mut body| {
                    let mut task = "";
                    for (mark, html) in [
                        ("[ ] ", "<input type=\"checkbox\" disabled> "),
                        ("[x] ", "<input type=\"checkbox\" disabled checked> "),
                        ("[X] ", "<input type=\"checkbox\" disabled checked> "),
                    ] {
                        if let Some(rest) = body[0].strip_prefix(mark) {
                            body[0] = rest.to_string();
                            task = html;
                            break;
                        }
                    }
                    let mut html = blocks(&body, ids);
                    if !loose && html.starts_with("<p>") {
                        if let Some(end) = html.find("</p>") {
                            html = format!("{}{}", &html[3..end], &html[end + 4..]);
                        }
                    }
                    if task.is_empty() {
                        format!("<li>{html}</li>")
                    } else if let Some(rest) = html.strip_prefix("<p>") {
                        format!("<li class=\"task\"><p>{task}{rest}</li>")
                    } else {
                        format!("<li class=\"task\">{task}{html}</li>")
                    }
                })
                .collect();
            let (tag, attr) = match (ordered, start) {
                (false, _) => ("ul", String::new()),
                (true, 1) => ("ol", String::new()),
                (true, n) => ("ol", format!(" start=\"{n}\"")),
            };
            out.push(format!("<{tag}{attr}>\n{}\n</{tag}>", lis.join("\n")));
            continue;
        }

        // Table
        if line.contains('|') && i + 1 < lines.len() {
            let head = cells(line);
            if let Some(aligns) = table_aligns(&lines[i + 1]).filter(|a| a.len() == head.len()) {
                let cell = |tag: &str, k: usize, text: &str| {
                    let style = match aligns[k] {
                        "" => String::new(),
                        a => format!(" style=\"text-align:{a}\""),
                    };
                    format!("<{tag}{style}>{}</{tag}>", inline(text))
                };
                let th: String = head.iter().enumerate().map(|(k, c)| cell("th", k, c)).collect();
                let mut rows = Vec::new();
                i += 2;
                while i < lines.len() && !is_blank(&lines[i]) && lines[i].contains('|') {
                    let row = cells(&lines[i]);
                    let tds: String = (0..head.len())
                        .map(|k| cell("td", k, row.get(k).map_or("", String::as_str)))
                        .collect();
                    rows.push(format!("<tr>{tds}</tr>"));
                    i += 1;
                }
                out.push(format!(
                    "<table>\n<thead><tr>{th}</tr></thead>\n<tbody>\n{}\n</tbody>\n</table>",
                    rows.join("\n")
                ));
                continue;
            }
        }

        // Paragraph
        let mut para = vec![line.trim_start()];
        i += 1;
        while i < lines.len() && !is_blank(&lines[i]) && !starts_block(&lines[i]) {
            para.push(lines[i].trim_start());
            i += 1;
        }
        out.push(format!("<p>{}</p>", inline(para.join("\n").trim_end())));
    }

    out.join("\n")
}

// ---------- Inline level: bold, links, code, etc. ----------

fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    inline_chars(&chars)
}

/// Length of the run of identical characters starting at `i`.
fn run_len(c: &[char], i: usize) -> usize {
    c[i..].iter().take_while(|x| **x == c[i]).count()
}

fn starts_with(c: &[char], i: usize, prefix: &str) -> bool {
    let mut k = i;
    prefix.chars().all(|p| {
        k += 1;
        c.get(k - 1) == Some(&p)
    })
}

fn starts_url(c: &[char], i: usize) -> bool {
    starts_with(c, i, "http://") || starts_with(c, i, "https://")
}

/// Position of the run of exactly `n` backticks that closes an inline code span.
fn find_code_close(c: &[char], from: usize, n: usize) -> Option<usize> {
    let mut j = from;
    while j < c.len() {
        if c[j] == '`' {
            let r = run_len(c, j);
            if r == n {
                return Some(j);
            }
            j += r;
        } else {
            j += 1;
        }
    }
    None
}

/// Skips past an escape or an inline code span; `None` if `c[j]` is neither.
fn skip_opaque(c: &[char], j: usize) -> Option<usize> {
    match c[j] {
        '\\' => Some(j + 2),
        '`' => {
            let n = run_len(c, j);
            Some(find_code_close(c, j + n, n).map_or(j + n, |e| e + n))
        }
        _ => None,
    }
}

struct Link {
    text: (usize, usize),
    url: String,
    title: Option<String>,
    end: usize,
}

/// Parses `[text](url "title")` starting from the `[` at position `open`.
fn parse_link(c: &[char], open: usize) -> Option<Link> {
    let mut depth = 0usize;
    let mut j = open;
    let close = loop {
        if j >= c.len() {
            return None;
        }
        if let Some(next) = skip_opaque(c, j) {
            j = next;
            continue;
        }
        if c[j] == '[' {
            depth += 1;
        } else if c[j] == ']' {
            depth -= 1;
            if depth == 0 {
                break j;
            }
        }
        j += 1;
    };
    if c.get(close + 1) != Some(&'(') {
        return None;
    }

    let mut j = close + 2;
    let mut url = String::new();
    let mut parens = 0;
    while j < c.len() && !c[j].is_whitespace() {
        match c[j] {
            '(' => parens += 1,
            ')' if parens == 0 => break,
            ')' => parens -= 1,
            _ => {}
        }
        url.push(c[j]);
        j += 1;
    }
    if url.is_empty() {
        return None;
    }
    let skip_spaces = |mut j: usize| {
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        j
    };
    j = skip_spaces(j);
    let mut title = None;
    if c.get(j) == Some(&'"') {
        let end = (j + 1..c.len()).find(|&k| c[k] == '"')?;
        title = Some(c[j + 1..end].iter().collect());
        j = skip_spaces(end + 1);
    }
    if c.get(j) != Some(&')') {
        return None;
    }
    Some(Link { text: (open + 1, close), url, title, end: j + 1 })
}

fn title_attr(title: &Option<String>) -> String {
    title.as_ref().map_or(String::new(), |t| format!(" title=\"{}\"", escape(t)))
}

/// Finds the closing delimiter (`*`, `**`, `_`, `__`, `~~`) starting at `from`.
fn find_closer(c: &[char], from: usize, ch: char, len: usize) -> Option<usize> {
    let mut j = from;
    while j < c.len() {
        if let Some(next) = skip_opaque(c, j) {
            j = next;
            continue;
        }
        if c[j] != ch {
            j += 1;
            continue;
        }
        let r = run_len(c, j);
        // In `**a *b***`, the closer is taken from the end of the run.
        let pos = match (len, r) {
            (2, 1) | (1, 2) => None,
            (2, _) => Some(j + r - 2),
            (_, _) => Some(j + r - 1),
        };
        let before_ok = j > from && !c[j - 1].is_whitespace();
        let after_ok = ch != '_' || !c.get(j + r).is_some_and(|a| a.is_alphanumeric());
        if let (Some(p), true, true) = (pos, before_ok, after_ok) {
            return Some(p);
        }
        j += r;
    }
    None
}

fn inline_chars(c: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < c.len() {
        let ch = c[i];
        match ch {
            // Escapes (\* \_ \# ...) and hard line break from a trailing \
            '\\' => match c.get(i + 1) {
                Some(&n) if n.is_ascii_punctuation() => {
                    push_escaped(&mut out, n);
                    i += 2;
                }
                Some('\n') => {
                    out.push_str("<br>\n");
                    i += 2;
                }
                _ => {
                    out.push('\\');
                    i += 1;
                }
            },

            // Inline code: nothing inside is interpreted
            '`' => {
                let n = run_len(c, i);
                if let Some(end) = find_code_close(c, i + n, n) {
                    let code: String = c[i + n..end].iter().collect();
                    out.push_str(&format!("<code>{}</code>", escape(code.trim())));
                    i = end + n;
                } else {
                    out.extend(std::iter::repeat('`').take(n));
                    i += n;
                }
            }

            // Image ![alt](url "title")
            '!' if c.get(i + 1) == Some(&'[') => match parse_link(c, i + 1) {
                Some(l) => {
                    let alt: String = c[l.text.0..l.text.1].iter().collect();
                    out.push_str(&format!(
                        "<img src=\"{}\" alt=\"{}\"{}>",
                        safe_url(&l.url),
                        escape(&alt),
                        title_attr(&l.title)
                    ));
                    i = l.end;
                }
                None => {
                    out.push('!');
                    i += 1;
                }
            },

            // Link [text](url "title")
            '[' => match parse_link(c, i) {
                Some(l) => {
                    out.push_str(&format!(
                        "<a href=\"{}\"{}>{}</a>",
                        safe_url(&l.url),
                        title_attr(&l.title),
                        inline_chars(&c[l.text.0..l.text.1])
                    ));
                    i = l.end;
                }
                None => {
                    out.push('[');
                    i += 1;
                }
            },

            // Autolink <https://...>
            '<' if starts_url(c, i + 1) => {
                let end = (i + 1..c.len()).find(|&k| c[k].is_whitespace() || c[k] == '<' || c[k] == '>');
                match end.filter(|&k| c[k] == '>') {
                    Some(end) => {
                        let url = escape(&c[i + 1..end].iter().collect::<String>());
                        out.push_str(&format!("<a href=\"{url}\">{url}</a>"));
                        i = end + 1;
                    }
                    None => {
                        out.push_str("&lt;");
                        i += 1;
                    }
                }
            }

            // Bare URL
            'h' if starts_url(c, i) && (i == 0 || !c[i - 1].is_alphanumeric()) => {
                let mut end = i;
                while end < c.len() && !c[end].is_whitespace() && c[end] != '<' {
                    end += 1;
                }
                // Trailing punctuation is not part of the URL.
                loop {
                    let last = c[end - 1];
                    let unbalanced = last == ')' && !c[i..end].contains(&'(');
                    if ".,;:!?*_~\"'".contains(last) || unbalanced {
                        end -= 1;
                    } else {
                        break;
                    }
                }
                let url = escape(&c[i..end].iter().collect::<String>());
                out.push_str(&format!("<a href=\"{url}\">{url}</a>"));
                i = end;
            }

            // Bold, italic, strikethrough
            '*' | '_' | '~' => {
                let r = run_len(c, i);
                let len = match (ch, r) {
                    ('~', 1) => 0,
                    (_, 1) => 1,
                    _ => 2,
                };
                // A _ inside a word (snake_case) opens nothing.
                let prev_ok = ch != '_' || i == 0 || !c[i - 1].is_alphanumeric();
                let next_ok = c.get(i + len).is_some_and(|n| !n.is_whitespace());
                let closer = if len > 0 && prev_ok && next_ok {
                    find_closer(c, i + len, ch, len)
                } else {
                    None
                };
                match closer {
                    Some(p) => {
                        let tag = match (ch, len) {
                            ('~', _) => "del",
                            (_, 2) => "strong",
                            _ => "em",
                        };
                        out.push_str(&format!("<{tag}>{}</{tag}>", inline_chars(&c[i + len..p])));
                        i = p + len;
                    }
                    None => {
                        let n = if len > 0 && prev_ok { len } else { r };
                        out.extend(std::iter::repeat(ch).take(n));
                        i += n;
                    }
                }
            }

            // Hard line break: two spaces at the end of a line
            ' ' => {
                let r = run_len(c, i);
                if r >= 2 && c.get(i + r) == Some(&'\n') {
                    out.push_str("<br>\n");
                    i += r + 1;
                } else {
                    out.extend(std::iter::repeat(' ').take(r));
                    i += r;
                }
            }

            _ => {
                push_escaped(&mut out, ch);
                i += 1;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn inline_text() {
        assert_eq!(
            render("**bold** *ital* ~~struck~~ `a*b*` snake_case_var __strong__ _soft_"),
            "<p><strong>bold</strong> <em>ital</em> <del>struck</del> <code>a*b*</code> \
             snake_case_var <strong>strong</strong> <em>soft</em></p>"
        );
        assert_eq!(render("**a *b* c** and ***d***"), "<p><strong>a <em>b</em> c</strong> and <strong><em>d</em></strong></p>");
        assert_eq!(render("2 * 3 * 4 and \\*not ital\\*"), "<p>2 * 3 * 4 and *not ital*</p>");
        assert_eq!(render("a  \nb"), "<p>a<br>\nb</p>");
    }

    #[test]
    fn links_and_images() {
        assert_eq!(
            render("[a *link*](https://a.b/c_(d) \"T\") ![alt](i.png)"),
            "<p><a href=\"https://a.b/c_(d)\" title=\"T\">a <em>link</em></a> <img src=\"i.png\" alt=\"alt\"></p>"
        );
        assert_eq!(
            render("see https://a.b/x_y_z. <https://c.d>"),
            "<p>see <a href=\"https://a.b/x_y_z\">https://a.b/x_y_z</a>. <a href=\"https://c.d\">https://c.d</a></p>"
        );
        assert_eq!(render("[x](javascript:alert(1))"), "<p><a href=\"#\">x</a></p>");
        assert_eq!(render("[not a link] (x)"), "<p>[not a link] (x)</p>");
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(render("<script>a && b</script>"), "<p>&lt;script&gt;a &amp;&amp; b&lt;/script&gt;</p>");
    }

    #[test]
    fn headings() {
        assert_eq!(
            render("# Café Ideas\n## C#\n# Café ideas ##\n#not a heading"),
            "<h1 id=\"cafe-ideas\">Café Ideas</h1>\n<h2 id=\"c\">C#</h2>\n\
             <h1 id=\"cafe-ideas-2\">Café ideas</h1>\n<p>#not a heading</p>"
        );
    }

    #[test]
    fn code_blocks() {
        assert_eq!(
            render("```rust\nlet a = \"<b>\";\n  **x**\n```\nend"),
            "<pre><code class=\"language-rust\">let a = &quot;&lt;b&gt;&quot;;\n  **x**\n</code></pre>\n<p>end</p>"
        );
        assert_eq!(render("~~~\nnot closed"), "<pre><code>not closed\n</code></pre>");
    }

    #[test]
    fn lists() {
        assert_eq!(
            render("- a\n- b\n  - c\n\n  more\n- [x] done\n- [ ] to do"),
            "<ul>\n<li>a</li>\n<li>b\n<ul>\n<li>c</li>\n</ul>\n<p>more</p></li>\n\
             <li class=\"task\"><input type=\"checkbox\" disabled checked> done</li>\n\
             <li class=\"task\"><input type=\"checkbox\" disabled> to do</li>\n</ul>"
        );
        assert_eq!(
            render("3. a\n\n4. b\n- c"),
            "<ol start=\"3\">\n<li><p>a</p></li>\n<li><p>b</p></li>\n</ol>\n<ul>\n<li>c</li>\n</ul>"
        );
        assert_eq!(render("1. a\n  - b"), "<ol>\n<li>a\n<ul>\n<li>b</li>\n</ul></li>\n</ol>");
    }

    #[test]
    fn quotes_and_rules() {
        assert_eq!(
            render("> a\nb\n> - c\n\n---\n* * *"),
            "<blockquote>\n<p>a\nb</p>\n<ul>\n<li>c</li>\n</ul>\n</blockquote>\n<hr>\n<hr>"
        );
    }

    #[test]
    fn tables() {
        assert_eq!(
            render("| a | b |\n|:-:|--:|\n| `x\\|y` | **2** |\n| alone |"),
            "<table>\n<thead><tr><th style=\"text-align:center\">a</th><th style=\"text-align:right\">b</th></tr></thead>\n\
             <tbody>\n<tr><td style=\"text-align:center\"><code>x\\|y</code></td><td style=\"text-align:right\"><strong>2</strong></td></tr>\n\
             <tr><td style=\"text-align:center\">alone</td><td style=\"text-align:right\"></td></tr>\n</tbody>\n</table>"
        );
        assert_eq!(render("a | b\nc"), "<p>a | b\nc</p>");
    }
}
