// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Rendering a value as the document states it.
//!
//! A document says `60 seconds` where the schema says `60000`, and `2500ms`
//! where it says `2500`, because a manual is written for a reader. The format
//! names which of those a span is claiming.

/// Render `value` the way format `name` states it.
///
/// `None` is the raw value, which is what a span with no format asks for. An
/// unknown format is an error rather than a pass-through: a typo in a format
/// name would otherwise make a span check nothing.
pub fn render(value: &str, name: Option<&str>) -> Result<String, String> {
    match name {
        None | Some("raw") => Ok(value.to_string()),
        Some("ms") => Ok(format!("{value}ms")),
        Some("seconds") => seconds(value),
        Some("code") => Ok(format!("`{value}`")),
        Some(other) => Err(format!(
            "unknown format '{other}' - known formats are raw, ms, seconds and code"
        )),
    }
}

/// A whole number of milliseconds as seconds, e.g. `60000` as `60 seconds`.
///
/// A value that is not a whole number of seconds has no rendering here: a
/// document saying `0.5 seconds` would be a second way of writing a duration,
/// and the one that exists is `ms`.
fn seconds(value: &str) -> Result<String, String> {
    let ms: u64 = value
        .parse()
        .map_err(|_| format!("'{value}' is not a number of milliseconds"))?;
    if !ms.is_multiple_of(1000) {
        return Err(format!(
            "{ms}ms is not a whole number of seconds - state it with the ms format"
        ));
    }
    let seconds = ms / 1000;
    Ok(format!("{seconds} seconds"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format_states_the_value_its_own_way() {
        assert_eq!(render("60000", None).unwrap(), "60000");
        assert_eq!(render("60000", Some("raw")).unwrap(), "60000");
        assert_eq!(render("2500", Some("ms")).unwrap(), "2500ms");
        assert_eq!(render("60000", Some("seconds")).unwrap(), "60 seconds");
        assert_eq!(render("100", Some("code")).unwrap(), "`100`");
    }

    #[test]
    fn an_unknown_format_is_an_error_not_a_pass_through() {
        let err = render("60000", Some("minutes")).unwrap_err();
        assert!(err.contains("minutes"), "{err}");
        assert!(err.contains("raw"), "{err}");
    }

    #[test]
    fn seconds_refuses_what_it_cannot_state() {
        // 575ms is a real constant, and there is no whole-seconds form of it.
        let err = render("575", Some("seconds")).unwrap_err();
        assert!(err.contains("whole number of seconds"), "{err}");

        // A string constant is not a duration.
        let err = render("OneROM", Some("seconds")).unwrap_err();
        assert!(err.contains("not a number"), "{err}");
    }
}
