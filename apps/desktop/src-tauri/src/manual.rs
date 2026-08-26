//! The manual, rendered for the window.
//!
//! The pages themselves live in [`ork_core::docs`], compiled into the binary
//! and shared with the command line -- which prints them as the Markdown they
//! are written in. Only the rendering is here, because only the window needs
//! it. That split is the same rule the rest of this application follows: the
//! content is in a shared crate, so nothing the window can show is unreachable
//! from a script.
//!
//! The HTML this produces is drawn with `{@html}` in the window. That is safe
//! here and would not be safe for anything else: this text is compiled into
//! the binary from files in the repository. It is not fetched, not
//! user-supplied, and not reachable by anything a scan found. No other screen
//! in this application renders HTML.

use pulldown_cmark::{Options, Parser, html};

use crate::commands::CmdResult;

/// One page, ready to draw.
#[derive(serde::Serialize)]
pub struct RenderedPage {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub html: String,
}

/// A page's identity, without its text.
#[derive(serde::Serialize)]
pub struct PageEntry {
    pub id: String,
    pub title: String,
    pub summary: String,
}

fn render(markdown: &str) -> String {
    // Tables and strikethrough are used throughout these pages; without them
    // the tables come out as rows of pipe characters. Footnotes and maths are
    // not used and are left off.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let mut out = String::with_capacity(markdown.len() + markdown.len() / 2);
    html::push_html(&mut out, Parser::new_ext(markdown, options));
    out
}

/// The list of pages, for a table of contents.
#[tauri::command]
pub fn manual_contents() -> CmdResult<Vec<PageEntry>> {
    Ok(ork_core::docs::contents()
        .into_iter()
        .map(|(id, title, summary)| PageEntry {
            id: id.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
        })
        .collect())
}

/// One page, rendered.
#[tauri::command]
pub fn manual_page(id: String) -> CmdResult<RenderedPage> {
    let page = ork_core::docs::find(&id).ok_or_else(|| format!("there is no page called {id}"))?;
    Ok(RenderedPage {
        id: page.id.to_string(),
        title: page.title.to_string(),
        summary: page.summary.to_string(),
        html: render(page.body),
    })
}

/// The licence, as plain text.
#[tauri::command]
pub fn manual_licence() -> CmdResult<String> {
    Ok(ork_core::docs::LICENCE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_survives_being_rendered() {
        // These pages are full of tables. Without the tables option they come
        // out as rows of pipe characters, which is worse than not showing the
        // page at all -- it looks like the tool is broken rather than like the
        // documentation is missing.
        let markdown = "| A | B |\n| --- | --- |\n| one | two |\n";
        let rendered = render(markdown);
        assert!(rendered.contains("<table>"), "{rendered}");
        assert!(rendered.contains("<td>one</td>"), "{rendered}");
        assert!(!rendered.contains("| one |"), "{rendered}");
    }

    #[test]
    fn code_and_headings_come_through() {
        let rendered = render("# Title\n\n```bash\noutlaw scan\n```\n");
        assert!(rendered.contains("<h1>Title</h1>"), "{rendered}");
        assert!(rendered.contains("<code"), "{rendered}");
        assert!(rendered.contains("outlaw scan"), "{rendered}");
    }

    #[test]
    fn every_real_page_renders_to_something_substantial() {
        for (id, _, _) in ork_core::docs::contents() {
            let page = manual_page(id.to_string()).expect("the page is there");
            assert!(
                page.html.len() > 200,
                "{id} rendered to {} bytes",
                page.html.len()
            );
            assert!(page.html.contains("<h1>"), "{id} has no heading");
        }
    }

    #[test]
    fn a_page_that_does_not_exist_is_an_error_rather_than_a_blank() {
        // A blank page reads as "there is nothing to say about this", which is
        // a different and wrong statement.
        assert!(manual_page("no-such-page".to_string()).is_err());
    }
}
