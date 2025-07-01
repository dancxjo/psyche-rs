use serde::Serialize;
use tinytemplate::TinyTemplate;

/// Renders a string template using `TinyTemplate`.
///
/// Template variables use the `{{name}}` syntax.
///
/// # Examples
///
/// ```
/// use psyche_rs::render_template;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Ctx { text: &'static str }
///
/// let out = render_template("Hello {{text}}!", &Ctx { text: "world" }).unwrap();
/// assert_eq!(out, "Hello world!");
/// ```
#[inline]
pub fn render_template<T: Serialize>(
    template: &str,
    ctx: &T,
) -> Result<String, tinytemplate::error::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("tpl", template)?;
    tt.render("tpl", ctx)
}

#[cfg(test)]
mod tests {
    use super::render_template;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Ctx<'a> {
        text: &'a str,
    }

    #[test]
    fn renders_variable() {
        let out = render_template("Hello {{text}}", &Ctx { text: "world" }).unwrap();
        assert_eq!(out, "Hello world");
    }
}
