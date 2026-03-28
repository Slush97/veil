//! URL extraction and link preview metadata parsing.
//!
//! Provides utilities to extract HTTP/HTTPS URLs from text and parse
//! OpenGraph / HTML meta tags from fetched pages. The actual HTTP fetch
//! is the caller's responsibility (keeps veil-core network-agnostic).

use linkify::{LinkFinder, LinkKind};

use crate::message::EmbedPreview;

/// Extract HTTP/HTTPS URLs from text.
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    finder
        .links(text)
        .filter(|link| {
            let url = link.as_str();
            url.starts_with("http://") || url.starts_with("https://")
        })
        .map(|link| link.as_str().to_string())
        .collect()
}

/// Parse OpenGraph and HTML meta tags from an HTML document.
///
/// Returns `None` if no useful metadata (title, description, or image)
/// could be extracted.
pub fn parse_embed_metadata(html: &str, url: &str) -> Option<EmbedPreview> {
    let document = scraper::Html::parse_document(html);

    let og_title = og_content(&document, "og:title");
    let og_desc = og_content(&document, "og:description");
    let og_image = og_content(&document, "og:image");
    let og_site = og_content(&document, "og:site_name");

    // Fallbacks to standard HTML tags
    let title = og_title.or_else(|| html_title(&document));
    let description = og_desc.or_else(|| meta_content(&document, "description"));

    if title.is_none() && description.is_none() && og_image.is_none() {
        return None;
    }

    Some(EmbedPreview {
        url: url.to_string(),
        title,
        description,
        image_url: og_image,
        site_name: og_site,
    })
}

/// Extract `content` attribute from `<meta property="og:..." content="...">`.
fn og_content(doc: &scraper::Html, property: &str) -> Option<String> {
    let selector = scraper::Selector::parse(&format!("meta[property=\"{property}\"]")).ok()?;
    doc.select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.to_string())
}

/// Extract text from `<title>...</title>`.
fn html_title(doc: &scraper::Html) -> Option<String> {
    let selector = scraper::Selector::parse("title").ok()?;
    doc.select(&selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .filter(|t| !t.is_empty())
}

/// Extract `content` attribute from `<meta name="..." content="...">`.
fn meta_content(doc: &scraper::Html, name: &str) -> Option<String> {
    let selector = scraper::Selector::parse(&format!("meta[name=\"{name}\"]")).ok()?;
    doc.select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_url() {
        let urls = extract_urls("Check out https://example.com today");
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn extract_multiple_urls() {
        let urls = extract_urls("Visit https://a.com and http://b.com/page?q=1");
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("a.com"));
        assert!(urls[1].contains("b.com"));
    }

    #[test]
    fn no_urls_returns_empty() {
        assert!(extract_urls("No links here").is_empty());
    }

    #[test]
    fn ignores_non_http_urls() {
        assert!(extract_urls("contact mailto:a@b.com or ftp://host").is_empty());
    }

    #[test]
    fn parse_full_og_tags() {
        let html = r#"
            <html><head>
                <meta property="og:title" content="My Page">
                <meta property="og:description" content="A cool page">
                <meta property="og:image" content="https://example.com/img.png">
                <meta property="og:site_name" content="Example">
                <title>Fallback Title</title>
            </head></html>
        "#;
        let preview = parse_embed_metadata(html, "https://example.com").unwrap();
        assert_eq!(preview.title.as_deref(), Some("My Page"));
        assert_eq!(preview.description.as_deref(), Some("A cool page"));
        assert_eq!(
            preview.image_url.as_deref(),
            Some("https://example.com/img.png")
        );
        assert_eq!(preview.site_name.as_deref(), Some("Example"));
    }

    #[test]
    fn fallback_to_html_title_and_meta_description() {
        let html = r#"
            <html><head>
                <title>My Page</title>
                <meta name="description" content="A cool page">
            </head></html>
        "#;
        let preview = parse_embed_metadata(html, "https://example.com").unwrap();
        assert_eq!(preview.title.as_deref(), Some("My Page"));
        assert_eq!(preview.description.as_deref(), Some("A cool page"));
        assert!(preview.image_url.is_none());
        assert!(preview.site_name.is_none());
    }

    #[test]
    fn no_metadata_returns_none() {
        let html = "<html><body><p>Hello</p></body></html>";
        assert!(parse_embed_metadata(html, "https://example.com").is_none());
    }

    #[test]
    fn malformed_html_does_not_panic() {
        let html = "<<<<not valid html>>>>";
        // Should return None, not panic
        let _ = parse_embed_metadata(html, "https://example.com");
    }
}
