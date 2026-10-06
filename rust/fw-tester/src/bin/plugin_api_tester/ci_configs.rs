// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Every config `ci/test-emu.sh` runs, and every config under
//! `onerom-config/test/`, runs through the plugin API tester in CI or is in
//! [`EXEMPT`] with the reason.
//!
//! A config added to either fails this test until it runs through the plugin
//! API tester or is in [`EXEMPT`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Configs the plugin API tester doesn't run in CI, each with the reason.
const EXEMPT: &[(&str, &str)] = &[(
    "onerom-config/test/24-multi-2332.json",
    "Doesn't build for any Fire 24 board - a 2332's select GPIOs and X pins aren't contiguous",
)];

/// The script that runs the configs.
const SCRIPT: &str = "ci/test-emu.sh";

const TEST_CONFIGS: &str = "onerom-config/test";

/// Whether `ci/test-emu.sh`'s `command` runs the plugin API tester.
fn runs_api_tester(command: &str) -> bool {
    let sized = command
        .strip_prefix("test_")
        .and_then(|rest| rest.strip_suffix("_config_api"))
        .is_some_and(|size| !size.is_empty() && size.bytes().all(|b| b.is_ascii_digit()));
    sized
        || matches!(
            command,
            "test_config_api" | "run_config_api" | "run_config_api_logging_off"
        )
}

/// `line` up to an unquoted `#` that starts a word.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut word_start = true;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '#' && word_start {
            return &line[..i];
        }
        word_start = c.is_whitespace();
    }
    line
}

/// The script's commands as words, without comments or quotes, with continued
/// lines joined.
fn commands(script: &str) -> Vec<Vec<String>> {
    let mut logical = Vec::new();
    let mut pending = String::new();
    for line in script.lines() {
        let code = strip_comment(line).trim_end();
        match code.strip_suffix('\\') {
            Some(continued) => {
                pending.push_str(continued);
                pending.push(' ');
            }
            None => {
                pending.push_str(code);
                logical.push(std::mem::take(&mut pending));
            }
        }
    }
    logical.push(pending);

    logical
        .iter()
        .flat_map(|line| line.split([';', '&', '|']))
        .map(|command| {
            command
                .split_whitespace()
                .map(|word| word.trim_matches(['"', '\'']).to_string())
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

fn is_config(word: &str) -> bool {
    word.starts_with("onerom-config/") && word.ends_with(".json")
}

/// Every config `script` runs, and those it runs through the plugin API tester.
fn script_configs(script: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut all = BTreeSet::new();
    let mut api = BTreeSet::new();
    for words in commands(script) {
        // `run_boards <runner> <config> <board>...` runs the config through
        // `runner`.
        let runner = match words.as_slice() {
            [first, runner, ..] if first == "run_boards" => runner,
            [first, ..] => first,
            [] => continue,
        };
        for config in words.iter().filter(|w| is_config(w)) {
            all.insert(config.clone());
            if runs_api_tester(runner) {
                api.insert(config.clone());
            }
        }
    }
    (all, api)
}

/// Every `.json` file under `dir`, as a path from `root`.
fn json_files(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let entries = std::fs::read_dir(root.join(dir))
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.join(dir).display()));
    for entry in entries {
        let path = dir.join(entry.expect("directory entry").file_name());
        if root.join(&path).is_dir() {
            json_files(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            out.insert(path.to_string_lossy().into_owned());
        }
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("project root")
}

#[test]
fn every_config_runs_the_api_tester_or_is_exempt() {
    let root = project_root();
    let script = std::fs::read_to_string(root.join(SCRIPT))
        .unwrap_or_else(|e| panic!("cannot read {SCRIPT}: {e}"));
    let (mut configs, api) = script_configs(&script);
    assert!(
        !api.is_empty(),
        "{SCRIPT} doesn't run the plugin API tester"
    );
    json_files(&root, Path::new(TEST_CONFIGS), &mut configs);

    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(config, _)| *config).collect();
    let mut problems = Vec::new();

    for config in &configs {
        if !api.contains(config) && !exempt.contains(config.as_str()) {
            problems.push(format!(
                "{config} doesn't run through the plugin API tester. Run it in {SCRIPT} with \
                 test_config_api, a test_<size>_config_api function or run_config_api, or add \
                 it to EXEMPT with the reason."
            ));
        }
    }

    for config in exempt {
        let stale = if api.contains(config) {
            Some(format!("runs through the plugin API tester in {SCRIPT}"))
        } else if !root.join(config).is_file() {
            Some("doesn't exist".to_string())
        } else if !configs.contains(config) {
            Some(format!("isn't in {TEST_CONFIGS} or run by {SCRIPT}"))
        } else {
            None
        };
        if let Some(why) = stale {
            problems.push(format!("{config} {why}. Remove it from EXEMPT."));
        }
    }

    assert!(
        problems.is_empty(),
        "\n{}\n\nEXEMPT is in {}.",
        problems.join("\n"),
        file!()
    );
}

#[test]
fn script_forms() {
    let script = r#"
# test_config_api fire-24-a onerom-config/test/commented.json
test_config_api fire-24-a onerom-config/test/board.json  # onerom-config/test/trailing.json
test_24_config_api "onerom-config/test/quoted.json"
test_24_config onerom-config/test/other.json
run_config_api_logging_off fire-32-a \
    onerom-config/test/continued.json
run_boards run_config_api onerom-config/test/boards.json $FIRE_24_BOARDS
echo "${x#y}"; test_config_api fire-24-a onerom-config/test/after.json
"#;
    let (all, api) = script_configs(script);
    let names = |set: &BTreeSet<String>| {
        set.iter()
            .map(|c| c.trim_start_matches("onerom-config/test/").to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&api),
        [
            "after.json",
            "board.json",
            "boards.json",
            "continued.json",
            "quoted.json"
        ]
    );
    assert_eq!(
        names(&all),
        [
            "after.json",
            "board.json",
            "boards.json",
            "continued.json",
            "other.json",
            "quoted.json"
        ]
    );
}
