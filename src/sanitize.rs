use std::collections::{HashMap, HashSet};

use ammonia::Builder;

pub fn resolve_cid_urls(html: &str, cid_map: &HashMap<String, String>) -> String {
    let mut result = html.to_string();
    for (cid, blob_id) in cid_map {
        let cid_ref = format!("cid:{cid}");
        let download_url = format!("/api/v1/attachments/{blob_id}");
        result = result.replace(&cid_ref, &download_url);
    }
    result
}

pub fn sanitize_email_html(raw_html: &str, cid_map: &HashMap<String, String>) -> String {
    let html = resolve_cid_urls(raw_html, cid_map);
    let generic_attributes = HashSet::from(["style", "class", "id"]);

    let style_properties = HashSet::from([
        "color",
        "background-color",
        "background",
        "font-family",
        "font-size",
        "font-weight",
        "font-style",
        "font-variant",
        "text-align",
        "text-decoration",
        "text-transform",
        "text-indent",
        "margin",
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
        "padding",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
        "border",
        "border-top",
        "border-right",
        "border-bottom",
        "border-left",
        "border-color",
        "border-style",
        "border-width",
        "width",
        "height",
        "max-width",
        "min-width",
        "max-height",
        "display",
        "vertical-align",
        "float",
        "list-style",
        "list-style-type",
        "table-layout",
        "border-collapse",
        "border-spacing",
        "line-height",
        "letter-spacing",
    ]);

    let mut builder = Builder::new();
    builder
        .add_tags([
            "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "colgroup", "col",
            "center", "div", "span", "img", "hr", "br",
        ])
        .add_generic_attributes(generic_attributes.iter())
        .add_tag_attributes(
            "table",
            &[
                "bgcolor",
                "width",
                "height",
                "cellpadding",
                "cellspacing",
                "border",
                "align",
            ],
        )
        .add_tag_attributes(
            "td",
            &[
                "bgcolor", "valign", "align", "width", "height", "colspan", "rowspan",
            ],
        )
        .add_tag_attributes(
            "th",
            &[
                "bgcolor", "valign", "align", "width", "height", "colspan", "rowspan",
            ],
        )
        .add_tag_attributes("tr", &["bgcolor", "valign", "align"])
        .add_tag_attributes("img", &["src", "alt", "width", "height"])
        .set_tag_attribute_value("a", "target", "_blank")
        .link_rel(Some("noopener noreferrer"))
        .id_prefix(Some("email-"))
        .strip_comments(true)
        .filter_style_properties(style_properties);

    builder.clean(&html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize(input: &str) -> String {
        sanitize_email_html(input, &HashMap::new())
    }

    #[test]
    fn strips_script_tags() {
        let result = sanitize(r#"<p>Hello</p><script>alert('xss')</script>"#);
        assert!(!result.contains("<script>"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn strips_event_handlers() {
        let result = sanitize(r#"<p onclick="alert('xss')">Hello</p>"#);
        assert!(!result.contains("onclick"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn allows_safe_html() {
        let result = sanitize(
            r#"<div style="color:red"><b>Bold</b> <a href="https://example.com">Link</a></div>"#,
        );
        assert!(result.contains("color:red"));
        assert!(result.contains("<b>Bold</b>"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn blocks_javascript_urls() {
        let result = sanitize(r#"<a href="javascript:alert('xss')">Click</a>"#);
        assert!(!result.contains("javascript:"));
    }

    #[test]
    fn sets_target_blank_on_links() {
        let result = sanitize(r#"<a href="https://example.com">Link</a>"#);
        assert!(result.contains(r#"target="_blank""#));
        assert!(result.contains("noopener noreferrer"));
    }

    #[test]
    fn prefixes_ids() {
        let result = sanitize(r#"<div id="main">Content</div>"#);
        assert!(result.contains(r#"id="email-main""#));
    }

    #[test]
    fn filters_dangerous_css_properties() {
        let result =
            sanitize(r#"<div style="color:red; position:fixed; z-index:9999">Content</div>"#);
        assert!(result.contains("color:red"));
        assert!(!result.contains("position"));
        assert!(!result.contains("z-index"));
    }

    #[test]
    fn strips_form_elements() {
        let result =
            sanitize(r#"<form action="/steal"><input type="text"><button>Submit</button></form>"#);
        assert!(!result.contains("<form"));
        assert!(!result.contains("<input"));
        assert!(!result.contains("<button"));
    }

    #[test]
    fn allows_table_attributes() {
        let result = sanitize(
            r##"<table bgcolor="#fff" cellpadding="5"><tr><td valign="top" align="center">Cell</td></tr></table>"##,
        );
        assert!(result.contains("bgcolor"));
        assert!(result.contains("cellpadding"));
        assert!(result.contains("valign"));
        assert!(result.contains("align"));
    }

    #[test]
    fn resolves_cid_urls() {
        let cid_map = HashMap::from([(
            "image001@example.com".to_string(),
            "blob-abc123".to_string(),
        )]);
        let input = r#"<img src="cid:image001@example.com" alt="logo">"#;
        let result = sanitize_email_html(input, &cid_map);
        assert!(result.contains("/api/v1/attachments/blob-abc123"));
        assert!(!result.contains("cid:"));
    }
}
