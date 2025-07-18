use std::{
    any::Any,
    fs,
    ops::Index,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Local, NaiveDateTime};
use eyre::Context;
use log::error;
use serde::Deserialize;
use std::fmt::Write as _;
use tiny_http::{Header, Response, ResponseBox};
use uri_rs::QueryParameters;

use crate::{CSS, NAME};

struct Config {}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    pub title:       String,
    pub datetime:    NaiveDateTime,
    pub description: Option<String>,
}

pub fn render(query: &QueryParameters) -> ResponseBox {
    let content_dir = dirs::config_dir()
        .expect("system should have a config dir")
        .join(NAME)
        .join("words");

    if let Some(Some(title)) = query.get("title") {
        dbg!(&title);
        render_document(&content_dir, &title)
    } else {
        render_index(&content_dir).unwrap_or_else(|e| {
            error!("Failed to render index: {e}");
            return Response::empty(500).boxed();
        })
    }
}

fn find_content(content_dir: &Path) -> eyre::Result<Vec<PathBuf>> {
    let mut content = vec![];
    for entry in fs::read_dir(content_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "md") {
            continue;
        }
        content.push(path);
    }
    Ok(content)
}

fn render_index(content_dir: &Path) -> eyre::Result<ResponseBox> {
    let mut index = vec![];
    let content = find_content(content_dir)?.into_iter().flat_map(|path| {
        let title = path.with_extension("");
        let Some(title) = title.file_name() else {
            return None;
        };

        let mut meta = Meta::default();
        meta.title = title.to_string_lossy().into_owned();
        Some((path, meta))
    });
    for (path, meta) in content {
        let contents = fs::read_to_string(&path).context(format!(
            "Failed to read the entirety of {path:?} into a string"
        ))?;
        let (_document, meta) = markdown_to_document(&contents, meta);
        index.push((meta.title, meta.datetime));
    }
    index.sort_by_key(|(_, t)| *t);

    const TITLE: &str = "Words";
    let mut out = String::new();

    writeln!(out, "<!DOCTYPE html>").unwrap();
    writeln!(out, r#"<html lang="en-US">"#).unwrap();
    writeln!(out, "<head>").unwrap();
    {
        writeln!(out, r#"<meta charset="utf-8" />"#).unwrap();
        writeln!(
            out,
            r#"<meta name="viewport" content="width=device-width, initial-scale=1">"#
        )
        .unwrap();
        writeln!(out, r#"<title>{TITLE}</title>"#).unwrap();
        writeln!(out, r#"<meta property="og:title" content="{TITLE}" />"#).unwrap();
        writeln!(out, "<style>\n{CSS}\n</style>").unwrap();
    }
    writeln!(out, "</head>").unwrap();

    writeln!(out, "<body>").unwrap();
    {
        writeln!(out, "<ol>").unwrap();
        {
            for (title, datetime) in index {
                let href = format!("/words?title={title}");
                writeln!(out, r#"<li><a href="{href}">{title}</a></li>"#).unwrap();
            }
        }
        writeln!(out, "</ol>").unwrap();
    }
    writeln!(out, "</body>").unwrap();
    writeln!(out, r#"</html>"#).unwrap();

    let response = Response::from_string(out)
        .with_header(
            "Content-Type: text/html"
                .parse::<Header>()
                .expect("vaild header"),
        )
        .boxed();
    Ok(response)
}

fn render_document(content_dir: &Path, title: &str) -> ResponseBox {
    let mut meta = Meta::default();
    let mut path = content_dir.join(format!("{title}.md"));
    if !path.exists() || title.contains('/') {
        return Response::empty(404).boxed();
    }
    let (document, _meta) = match fs::read_to_string(&path) {
        Ok(contents) => markdown_to_document(&contents, meta),
        Err(e) => {
            error!("Failed to read the enirety of {path:?} into a string: {e}");
            return Response::empty(500).boxed();
        }
    };
    let response = Response::from_string(document)
        .with_header(
            "Content-Type: text/html"
                .parse::<Header>()
                .expect("vaild header"),
        )
        .boxed();
    response
}

fn markdown_to_document(contents: &str, mut meta: Meta) -> (String, Meta) {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    use std::sync::LazyLock;
    use syntect::{
        highlighting::{Theme, ThemeSet},
        parsing::SyntaxSet,
    };
    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
    static THEME: LazyLock<Theme> = LazyLock::new(|| {
        let theme_set = ThemeSet::load_defaults();
        theme_set.themes["base16-ocean.dark"].clone()
    });

    #[derive(Default)]
    enum ParseState {
        #[default]
        Normal,
        Meta,
        Highlight,
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);

    let mut state = ParseState::default();
    let mut code = String::new();
    let mut syntax = SYNTAX_SET.find_syntax_plain_text();
    let parser = Parser::new_ext(contents, options).filter_map(|event| match event {
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
            let lang = lang.trim();
            if lang == "frontmatter" {
                state = ParseState::Meta;
                None
            } else {
                state = ParseState::Highlight;
                syntax = SYNTAX_SET
                    .find_syntax_by_token(lang)
                    .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
                // Wrap code blocks in a div
                Some(Event::Html(r#"<div class="md-codeblock">"#.into()))
            }
        }
        Event::Text(text) => match state {
            ParseState::Normal => Some(Event::Text(text)),
            ParseState::Meta => {
                match toml::de::from_str::<Meta>(&text) {
                    Ok(m) => meta = m,
                    Err(e) => error!("Failed to parse metadata: {e}"),
                }
                None
            }
            ParseState::Highlight => {
                code.push_str(&text);
                None
            }
        },
        Event::End(TagEnd::CodeBlock) => match state {
            ParseState::Normal => Some(Event::End(TagEnd::CodeBlock)),
            ParseState::Meta => {
                state = ParseState::Normal;
                None
            }
            ParseState::Highlight => {
                let mut html =
                    syntect::html::highlighted_html_for_string(&code, &SYNTAX_SET, syntax, &THEME)
                        .unwrap_or(code.clone());
                html.push_str("</div>");

                code.clear();
                state = ParseState::Normal;
                Some(Event::Html(html.into()))
            }
        },
        _ => Some(event),
    });

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);

    let html = apply_document_template(&html, &meta);
    (html, meta)
}

fn apply_document_template(html: &str, meta: &Meta) -> String {
    let title = meta.title.as_str();

    let mut out = String::new();

    writeln!(out, "<!DOCTYPE html>").unwrap();
    writeln!(out, r#"<html lang="en-US">"#).unwrap();
    writeln!(out, "<head>").unwrap();
    {
        writeln!(out, r#"<meta charset="utf-8" />"#).unwrap();
        writeln!(
            out,
            r#"<meta name="viewport" content="width=device-width, initial-scale=1">"#
        )
        .unwrap();
        writeln!(out, r#"<title>{title}</title>"#).unwrap();
        writeln!(out, r#"<meta property="og:title" content="{title}" />"#).unwrap();

        if let Some(description) = meta.description.as_deref() {
            writeln!(
                out,
                r#"<meta name="description" content="{description}" />"#
            )
            .unwrap();
            writeln!(
                out,
                r#"<meta property="og:description" content="{description}" />"#
            )
            .unwrap();
        }
        writeln!(out, "<style>\n{CSS}\n</style>").unwrap();
    }
    writeln!(out, "</head>").unwrap();

    writeln!(out, r#"<body class="md-body">"#).unwrap();
    {
        writeln!(out, r#"<article class=".md-content-container">"#).unwrap();
        writeln!(out, r#"
            <div style="display: flex; justify-content: space-between; align-items: center; margin: 0">
                <h1 class="md-title" style="margin: 0; margin-bottom: 0.17ex;">{title}</h1>
                <span style="font-size: x-small; color: #999">{date}</span>
            </div>
            "#, date = meta.datetime).unwrap();
        // writeln!(out, r#"<h1 style="margin:0.17ex;">{title}</h1>"#).unwrap();
        // writeln!(out, r#"<p style="font-size: x-small; padding: 0; margin: 0.17ex; margin-top: 0; margin-bottom: 1ex">{date}</p>"#, date = meta.datetime).unwrap();
        writeln!(out, r#"<hr style="margin-bottom:2ex"/>"#).unwrap();
        writeln!(out, "{html}").unwrap();
        writeln!(out, "</article>").unwrap();
    }
    writeln!(out, "</body>").unwrap();

    writeln!(out, "</html>").unwrap();
    out
}
