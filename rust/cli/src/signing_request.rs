// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A community signing request: a GitHub issue asking for piers.rocks's
//! Community signing key to sign a One ROM's commissioning instance.
//!
//! `hardware request-signature` prints a link that opens the issue form
//! filled in. `scripts/sign-request.sh` reads the issue, signs it with
//! `hardware sign` and answers it. The form and the script contain copies of
//! these values, and this module's tests compare them.

use onerom_app::BoardSize;
use onerom_config::hw::Board;
use onerom_metadata::otp::format_chip_id;

/// The manufacturer of every community signing request.
pub const MANUFACTURER: &str = "onerom.org";

/// The ID of piers.rocks's Community signing key, which signs every
/// community signing request.
pub const KEY_ID: u16 = 2;

/// The issue form's file in `.github/ISSUE_TEMPLATE`.
pub const TEMPLATE: &str = "signing-request.yml";

/// The start of every signing request's title. The issue form's `title`
/// contains it too.
pub const TITLE: &str = "[signing request]";

/// The address that opens a new issue in One ROM's GitHub repository.
pub const NEW_ISSUE_URL: &str = "https://github.com/piersfinlayson/one-rom/issues/new";

/// A One ROM's community signing request.
pub struct SigningRequest {
    /// CHIPID, rows `0x000`–`0x003` read with ECC.
    pub chip_id: [u16; 4],
    pub board: Board,
    pub size: BoardSize,
}

impl SigningRequest {
    /// The request's fields in the issue form's order, each as its ID in the
    /// form and its value.
    pub fn fields(&self) -> [(&'static str, String); 4] {
        [
            ("chip_id", format_chip_id(self.chip_id)),
            ("board", self.board.name().to_string()),
            ("size", self.size.to_string()),
            ("manufacturer", MANUFACTURER.to_string()),
        ]
    }

    /// The link that opens the issue form filled in with the request. A
    /// query parameter fills in the field whose ID it has. The title carries
    /// the Chip ID so requests are told apart in the issue list.
    pub fn link(&self) -> String {
        let title = format!("{TITLE} {}", format_chip_id(self.chip_id));
        let query: Vec<String> = [("template", TEMPLATE.to_string()), ("title", title)]
            .into_iter()
            .chain(self.fields())
            .map(|(id, value)| format!("{id}={}", percent_encode(&value)))
            .collect();
        format!("{NEW_ISSUE_URL}?{}", query.join("&"))
    }
}

/// `value` as a URL's query value. Each byte other than an unreserved
/// character is percent-encoded.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::test_board::CHIP_ID;

    /// The script answering a signing request, from the repository's root.
    const SCRIPT: &str = "scripts/sign-request.sh";

    /// The directory of GitHub's issue forms, from the repository's root.
    const ISSUE_FORMS: &str = ".github/ISSUE_TEMPLATE";

    /// The repository's root.
    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read(path: PathBuf) -> String {
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Decodes `value`, a query value [`percent_encode`] wrote.
    fn percent_decode(value: &str) -> String {
        let mut bytes = Vec::new();
        let mut rest = value.as_bytes();
        while let Some((&byte, after)) = rest.split_first() {
            if byte == b'%' {
                let hex = std::str::from_utf8(&after[..2]).unwrap();
                bytes.push(u8::from_str_radix(hex, 16).unwrap());
                rest = &after[2..];
            } else {
                bytes.push(byte);
                rest = after;
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    fn request(board: &str, size: BoardSize) -> SigningRequest {
        SigningRequest {
            chip_id: CHIP_ID,
            board: Board::try_from_str(board).unwrap(),
            size,
        }
    }

    /// The link's address and its query parameters, decoded.
    fn parts(link: &str) -> (&str, Vec<(String, String)>) {
        let (address, query) = link.split_once('?').unwrap();
        let parameters = query
            .split('&')
            .map(|parameter| {
                let (id, value) = parameter.split_once('=').unwrap();
                (id.to_string(), percent_decode(value))
            })
            .collect();
        (address, parameters)
    }

    #[test]
    fn a_link_opens_the_form_filled_in_with_the_request() {
        let link = request("fire-40-a", BoardSize::L).link();
        let (address, parameters) = parts(&link);
        assert_eq!(address, NEW_ISSUE_URL);
        let title = format!("{TITLE} DE3F9C232F655B6B");
        let expected: Vec<(String, String)> = [
            ("template", TEMPLATE),
            ("title", title.as_str()),
            ("chip_id", "DE3F9C232F655B6B"),
            ("board", "fire-40-a"),
            ("size", "L"),
            ("manufacturer", MANUFACTURER),
        ]
        .iter()
        .map(|(id, value)| (id.to_string(), value.to_string()))
        .collect();
        assert_eq!(parameters, expected);
    }

    #[test]
    fn a_query_value_is_percent_encoded() {
        for value in ["onerom.org", "a b&c=d", "50%", "Café", "~_-."] {
            let encoded = percent_encode(value);
            assert!(
                encoded
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)),
                "{encoded}"
            );
            assert_eq!(percent_decode(&encoded), value);
        }
        // Unreserved characters are left as they are.
        assert_eq!(percent_encode("onerom.org"), "onerom.org");
    }

    /// An input field of an issue form.
    #[derive(Debug)]
    struct FormField {
        id: String,
        label: String,
        /// Its default value.
        value: Option<String>,
    }

    /// `text` without the quotes around a YAML string.
    fn unquote(text: &str) -> String {
        text.trim_matches(|c| c == '"' || c == '\'').to_string()
    }

    /// The input fields of the issue form `form`, in order. It reads the
    /// three keys it requires from the form's lines rather than parsing its
    /// YAML.
    fn input_fields(form: &str) -> Vec<FormField> {
        form.split("\n  - type: ")
            .skip(1)
            .filter(|item| item.starts_with("input"))
            .map(|item| {
                let value = |key: &str| {
                    item.lines().find_map(|line| {
                        let rest = line.trim().strip_prefix(key)?.strip_prefix(": ")?;
                        Some(unquote(rest))
                    })
                };
                FormField {
                    id: value("id").unwrap_or_else(|| panic!("a field without an id:{item}")),
                    label: value("label")
                        .unwrap_or_else(|| panic!("a field without a label:{item}")),
                    value: value("value"),
                }
            })
            .collect()
    }

    /// The value the script `script` assigns to the shell variable `name`.
    fn assigned<'a>(script: &'a str, name: &str) -> Option<&'a str> {
        script.lines().find_map(|line| {
            let value = line.strip_prefix(name)?.strip_prefix('=')?;
            Some(value.trim_matches('"'))
        })
    }

    /// The issue form, the script and the CLI each contain the request's
    /// values. Nothing else compares the copies.
    #[test]
    fn the_form_and_the_script_agree_with_the_cli() {
        let form = read(root().join(ISSUE_FORMS).join(TEMPLATE));
        let script = read(root().join(SCRIPT));

        // The form's title starts with [`TITLE`], as the link's does.
        let title = form
            .lines()
            .find_map(|line| line.strip_prefix("title: "))
            .map(unquote);
        assert!(
            title
                .as_deref()
                .is_some_and(|title| title.starts_with(TITLE)),
            "{title:?}"
        );

        // The script reaches GitHub through curl. It doesn't run gh or read
        // gh's GH_ variables, as gh isn't installed where it runs.
        let code = script
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'));
        for line in code {
            let mut words =
                line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
            assert!(
                !words.any(|word| word == "gh" || word.starts_with("GH_")),
                "{line}"
            );
        }

        // The form's fields are the request's, in its order. The link fills
        // them in by their IDs.
        let fields = input_fields(&form);
        let ids: Vec<&str> = fields.iter().map(|field| field.id.as_str()).collect();
        let request = request("fire-24-f", BoardSize::M);
        let expected: Vec<&str> = request.fields().iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, expected);

        // The form fills in the manufacturer, which the script insists on.
        let manufacturer = fields.iter().find(|field| field.id == "manufacturer");
        let default = manufacturer.and_then(|field| field.value.as_deref());
        assert_eq!(default, Some(MANUFACTURER));
        assert_eq!(assigned(&script, "MANUFACTURER"), Some(MANUFACTURER));
        assert_eq!(
            assigned(&script, "KEY_ID"),
            Some(KEY_ID.to_string().as_str())
        );

        // The script reads the issues NEW_ISSUE_URL opens.
        let repo = assigned(&script, "REPO").expect("the script assigns REPO");
        assert_eq!(
            NEW_ISSUE_URL,
            format!("https://github.com/{repo}/issues/new")
        );

        // The script finds each field's value beneath its label, as GitHub
        // writes an issue's body. Each label is a variable named after the
        // field's ID.
        for field in &fields {
            let name = format!("{}_LABEL", field.id.to_uppercase());
            assert_eq!(
                assigned(&script, &name),
                Some(field.label.as_str()),
                "{name}"
            );
        }
    }

    /// The built-in signing key table lets the Community key sign a
    /// community signing request's manufacturer.
    #[test]
    fn the_community_key_may_sign_the_community_manufacturer() {
        let table = onerom_app::SignerTable::built_in();
        let signer = table.get(KEY_ID).expect("the table contains KEY_ID");
        assert!(signer.allows(MANUFACTURER));
    }
}
