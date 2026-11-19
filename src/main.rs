//! Generates the site: notes/*.md -> dist/*.html (+ a home page listing the notes).
//!
//!   cargo run              builds the site into dist/
//!   cargo run -- --serve   serves the site at http://localhost:8000 (rebuilt on every request)
//!
//! The notes folder is `notes/` by default; set the IMDSRC environment variable
//! to read them from another path (e.g. `IMDSRC=sekhmet-docs`).

mod markdown;

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use std::{env, fs, thread};

use markdown::{escape, render};

const SITE_TITLE: &str = "Cogito, ergo scribo";
const INDEX_INTRO: &str = "A thought is only half-formed until it meets the page; writing is how it learns what it meant.";
const DEFAULT_NOTES: &str = "notes";
const NOTES_ENV: &str = "IMDSRC";
const DIST: &str = "dist";
const STYLE: &str = "style.css";

fn main() {
    if env::args().any(|a| a == "--serve") {
        if let Err(e) = serve() {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    } else {
        match build() {
            Ok(n) => println!("{n} note(s) generated in {DIST}/"),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

// ---------- Generation ----------

struct Note {
    title: String,
    href: String,
    modified: u64,
}

/// A top-level note named `folder.name.md` belongs to the virtual folder `folder`
/// (one level only: `a.b.c.md` is note `b.c` in folder `a`).
fn split_folder(stem: &str) -> Option<(&str, &str)> {
    if stem.contains('/') {
        return None;
    }
    stem.split_once('.').filter(|(f, n)| !f.is_empty() && !n.is_empty())
}

/// Output path (without `.html`) of a note stem.
fn output_stem(stem: &str) -> String {
    match split_folder(stem) {
        Some((folder, name)) => format!("{folder}/{name}"),
        None => stem.to_string(),
    }
}

fn note_list(notes: &[Note], base: &str) -> String {
    let items: Vec<String> = notes
        .iter()
        .map(|n| {
            let href = n.href.strip_prefix(base).unwrap_or(&n.href);
            format!(
                "<li><a href=\"{}\">{}</a> <time>{}</time></li>",
                url_encode(href),
                escape(&n.title),
                format_date(n.modified)
            )
        })
        .collect();
    format!("<ul class=\"notes\">\n{}\n</ul>", items.join("\n"))
}

/// Recursively lists the files in `dir`, as relative paths using `/`.
fn walk(dir: &Path, base: &str, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if base.is_empty() { name } else { format!("{base}/{name}") };
        if entry.file_type()?.is_dir() {
            walk(&entry.path(), &rel, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// Builds a page. `trail` lists the intermediate levels between the home page and
/// this page as (label, href relative to the site root); the last one is the parent.
/// The home page passes `None`.
fn page(title: &str, body: &str, root: &str, trail: Option<&[(&str, &str)]>) -> String {
    let mut crumbs = format!("<a href=\"{root}index.html\">{}</a>", escape(&hostname()));
    let mut back = String::new();
    if let Some(trail) = trail {
        for (label, href) in trail {
            crumbs.push_str(&format!(
                "<span class=\"sep\">/</span><a href=\"{root}{}\">{}</a>",
                url_encode(href),
                escape(label)
            ));
        }
        crumbs.push_str(&format!(
            "<span class=\"sep\">/</span><span class=\"current\">{}</span>",
            escape(title)
        ));
        let parent = trail.last().map_or("index.html".to_string(), |(_, h)| url_encode(h));
        back = format!(
            "<a class=\"back\" href=\"{root}{parent}\" title=\"Back (Esc)\" aria-label=\"Back\">←</a>"
        );
    }
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="{root}style.css">
</head>
<body>
<header>{back}<nav class="crumbs">{crumbs}</nav></header>
<main>
{body}
</main>
<script>
// Smart back: return in history when we came from this site (keeps the scroll
// position), otherwise go up to the parent page. Esc does the same.
(function () {{
  var back = document.querySelector("header .back");
  if (!back) return;
  function goBack(e) {{
    var ref = document.referrer;
    if (ref && new URL(ref).origin === location.origin && history.length > 1) {{
      if (e) e.preventDefault();
      history.back();
    }} else if (!e) {{
      location.href = back.href;
    }}
  }}
  back.addEventListener("click", goBack);
  document.addEventListener("keydown", function (e) {{
    if (e.key === "Escape" && !e.ctrlKey && !e.metaKey && !e.altKey) goBack(null);
  }});
}})();
</script>
</body>
</html>
"#,
        title = escape(title),
    )
}

/// Name of the machine building the site, shown in the page header.
fn hostname() -> String {
    let clean = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    fs::read_to_string("/etc/hostname")
        .ok()
        .and_then(clean)
        .or_else(|| env::var("HOSTNAME").ok().and_then(clean))
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            clean(String::from_utf8_lossy(&out.stdout).into_owned())
        })
        .unwrap_or_else(|| "localhost".to_string())
}

/// Links between notes (`[see](other.md)`) point to the generated page.
/// `root` is the path from the current page back to the site root; it is used
/// for flat links (no `/`) so `folder.name.md` resolves from anywhere.
fn rewrite_md_links(html: &str, root: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(p) = rest.find("href=\"") {
        let start = p + 6;
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('"') else { break };
        let url = &rest[..end];
        let (path, fragment) = url.split_at(url.find('#').unwrap_or(url.len()));
        match path.strip_suffix(".md") {
            Some(stem) if !url.contains(':') => {
                if !stem.contains('/') {
                    out.push_str(root);
                    out.push_str(&output_stem(stem));
                } else {
                    out.push_str(stem);
                }
                out.push_str(".html");
                out.push_str(fragment);
            }
            _ => out.push_str(url),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn url_encode(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Date yyyy-mm-dd (UTC) from a number of seconds since 1970.
fn format_date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

fn write_file(dest: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest, content)
}

/// Builds the whole site into `dist/` and returns the number of notes.
fn build() -> io::Result<usize> {
    let notes_dir = match env::var_os(NOTES_ENV).filter(|v| !v.is_empty()) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if !dir.is_dir() {
                let msg = format!("{NOTES_ENV}={} is not a folder", dir.display());
                return Err(io::Error::new(io::ErrorKind::NotFound, msg));
            }
            dir
        }
        None => PathBuf::from(DEFAULT_NOTES),
    };

    let dist = Path::new(DIST);
    if dist.exists() {
        fs::remove_dir_all(dist)?;
    }
    fs::create_dir_all(dist)?;
    fs::copy(STYLE, dist.join("style.css"))?;

    let mut files = Vec::new();
    if notes_dir.is_dir() {
        walk(&notes_dir, "", &mut files)?;
    }

    let mut notes = Vec::new();
    for rel in files {
        let src = notes_dir.join(&rel);
        let Some(stem) = rel.strip_suffix(".md") else {
            // Images and other files: copied as-is.
            write_file(&dist.join(&rel), &fs::read(&src)?)?;
            continue;
        };
        let md = fs::read_to_string(&src)?;
        let title = md
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                let out = output_stem(stem);
                out.rsplit('/').next().unwrap_or(&out).to_string()
            });
        let href = format!("{}.html", output_stem(stem));
        let root = "../".repeat(href.matches('/').count());
        // Flat links in a virtual-folder note resolve from the notes root.
        let link_root = if split_folder(stem).is_some() { root.as_str() } else { "" };
        let body = format!(
            "<article>\n{}\n</article>",
            rewrite_md_links(&render(&md), link_root)
        );
        let trail: Vec<(&str, String)> = match split_folder(stem) {
            Some((folder, _)) => vec![(folder, format!("{folder}/index.html"))],
            None => Vec::new(),
        };
        let trail: Vec<(&str, &str)> = trail.iter().map(|(l, h)| (*l, h.as_str())).collect();
        write_file(&dist.join(&href), page(&title, &body, &root, Some(&trail)).as_bytes())?;
        let modified = fs::metadata(&src)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        notes.push(Note { title, href, modified });
    }

    notes.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.title.cmp(&b.title)));
    let count = notes.len();

    // Group virtual-folder notes; each folder gets its own index page.
    let mut folders: Vec<(String, Vec<Note>)> = Vec::new();
    let mut top = Vec::new();
    for n in notes {
        let stem = n.href.strip_suffix(".html").unwrap_or(&n.href);
        let folder = stem.split_once('/').map(|(f, _)| f.to_string());
        match folder {
            Some(f) if n.href.matches('/').count() == 1 && is_virtual(&f, &notes_dir) => {
                match folders.iter_mut().find(|(name, _)| *name == f) {
                    Some((_, v)) => v.push(n),
                    None => folders.push((f, vec![n])),
                }
            }
            _ => top.push(n),
        }
    }
    for (name, list) in &folders {
        let body = format!(
            "<h1>{}</h1>\n{}",
            escape(name),
            note_list(list, &format!("{name}/"))
        );
        write_file(&dist.join(name).join("index.html"), page(name, &body, "../", Some(&[])).as_bytes())?;
        top.push(Note {
            title: format!("{name}/"),
            href: format!("{name}/index.html"),
            modified: list.iter().map(|n| n.modified).max().unwrap_or(0),
        });
    }
    top.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.title.cmp(&b.title)));

    let list = if top.is_empty() {
        format!(
            "<p>No notes yet. Add a <code>.md</code> file to <code>{}</code>.</p>",
            escape(&notes_dir.display().to_string())
        )
    } else {
        note_list(&top, "")
    };
    let body = format!(
        "<h1>{}</h1>\n<p>{}</p>\n{list}",
        escape(SITE_TITLE),
        escape(INDEX_INTRO)
    );
    fs::write(dist.join("index.html"), page(SITE_TITLE, &body, "", None))?;
    Ok(count)
}

/// True if `folder` comes from `folder.*.md` files at the top of the notes
/// directory (rather than a real sub-directory).
fn is_virtual(folder: &str, notes_dir: &Path) -> bool {
    fs::read_dir(notes_dir).into_iter().flatten().flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.strip_suffix(".md")
            .and_then(split_folder)
            .is_some_and(|(f, _)| f == folder)
    })
}

// ---------- Local server ----------

const TEXT: &str = "text/plain; charset=utf-8";

fn serve() -> io::Result<()> {
    let port = env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000u16);
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("Site served at http://localhost:{port}");

    static BUILD_LOCK: Mutex<()> = Mutex::new(());
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            let _ = handle(stream, &BUILD_LOCK);
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream, lock: &Mutex<()>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..n]);
    let target = request.split_whitespace().nth(1).unwrap_or("/");
    let path = percent_decode(target.split(['?', '#']).next().unwrap_or("/"));

    let (status, content_type, body) = respond(&path, lock);
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

fn respond(path: &str, lock: &Mutex<()>) -> (&'static str, &'static str, Vec<u8>) {
    let not_found = ("404 Not Found", TEXT, "404 — page not found".as_bytes().to_vec());
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = build() {
        return ("500 Internal Server Error", TEXT, format!("Error: {e}").into_bytes());
    }

    let mut file = PathBuf::from(DIST);
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return not_found,
            p if p.contains('\\') => return not_found,
            p => file.push(p),
        }
    }
    if file.is_dir() {
        file.push("index.html");
    }
    let content_type = match file.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };
    match fs::read(&file) {
        Ok(body) => ("200 OK", content_type, body),
        Err(_) => not_found,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) if b[i] == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
