use std::collections::HashSet;

use ammonia::Builder;

pub fn sanitize_email_html(raw_html: &str) -> String {
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
        .add_tags(["table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "colgroup", "col", "center", "div", "span", "img", "hr", "br"])
        .add_generic_attributes(generic_attributes.iter())
        .add_tag_attributes("table", &["bgcolor", "width", "height", "cellpadding", "cellspacing", "border", "align"])
        .add_tag_attributes("td", &["bgcolor", "valign", "align", "width", "height", "colspan", "rowspan"])
        .add_tag_attributes("th", &["bgcolor", "valign", "align", "width", "height", "colspan", "rowspan"])
        .add_tag_attributes("tr", &["bgcolor", "valign", "align"])
        .add_tag_attributes("img", &["src", "alt", "width", "height"])
        .set_tag_attribute_value("a", "target", "_blank")
        .link_rel(Some("noopener noreferrer"))
        .id_prefix(Some("email-"))
        .strip_comments(true)
        .filter_style_properties(style_properties);

    builder.clean(raw_html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_tags() {
        let input = r#"<p>Hello</p><script>alert('xss')</script>"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("<script>"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn strips_event_handlers() {
        let input = r#"<p onclick="alert('xss')">Hello</p>"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("onclick"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn allows_safe_html() {
        let input = r#"<div style="color:red"><b>Bold</b> <a href="https://example.com">Link</a></div>"#;
        let result = sanitize_email_html(input);
        assert!(result.contains("color:red"));
        assert!(result.contains("<b>Bold</b>"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn blocks_javascript_urls() {
        let input = r#"<a href="javascript:alert('xss')">Click</a>"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("javascript:"));
    }

    #[test]
    fn sets_target_blank_on_links() {
        let input = r#"<a href="https://example.com">Link</a>"#;
        let result = sanitize_email_html(input);
        assert!(result.contains(r#"target="_blank""#));
        assert!(result.contains("noopener noreferrer"));
    }

    #[test]
    fn prefixes_ids() {
        let input = r#"<div id="main">Content</div>"#;
        let result = sanitize_email_html(input);
        assert!(result.contains(r#"id="email-main""#));
    }

    #[test]
    fn filters_dangerous_css_properties() {
        let input = r#"<div style="color:red; position:fixed; z-index:9999">Content</div>"#;
        let result = sanitize_email_html(input);
        assert!(result.contains("color:red"));
        assert!(!result.contains("position"));
        assert!(!result.contains("z-index"));
    }

    #[test]
    fn strips_form_elements() {
        let input = r#"<form action="/steal"><input type="text"><button>Submit</button></form>"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("<form"));
        assert!(!result.contains("<input"));
        assert!(!result.contains("<button"));
    }

    #[test]
    fn allows_table_attributes() {
        let input = r##"<table bgcolor="#fff" cellpadding="5"><tr><td valign="top" align="center">Cell</td></tr></table>"##;
        let result = sanitize_email_html(input);
        assert!(result.contains("bgcolor"));
        assert!(result.contains("cellpadding"));
        assert!(result.contains("valign"));
        assert!(result.contains("align"));
    }
}
