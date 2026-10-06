// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `hardware sign`.

use std::path::Path;

use onerom_app::SignerTable;
use onerom_cli::Options;
use onerom_cli::signing::KeyFile;

use super::{
    Keyboard, SIGNER, Screen, Server, acme_signing, failed, hardware_of, pin_prompt, refused_lines,
    shell_line, signature_hex, written_failure,
};
use crate::args::hardware::{HardwareCommands, HardwareSignArgs};
use crate::hardware::{
    check_key_allows, check_server_key, key_source, shell_word, sign_values, signer_for,
    signer_with_id, signing_key,
};
use crate::test_board::{key_file, table, table_allowing};

/// `line`, which starts `onerom hardware sign`, parsed and checked as the CLI
/// does. `key.pem` stands for `key`. Returns the arguments and the options.
fn sign_args_of(line: &str, key: &Path) -> (HardwareSignArgs, Options) {
    let key = key.to_str().unwrap();
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| if word == "key.pem" { key } else { word })
        .collect();
    args_of(&words)
}

/// `words`, which start `onerom hardware sign`, parsed and checked as the CLI
/// does. Returns the arguments and the options.
fn args_of(words: &[&str]) -> (HardwareSignArgs, Options) {
    let (command, options) = hardware_of(words);
    let HardwareCommands::Sign(args) = command else {
        panic!("not hardware sign");
    };
    (args, options)
}

/// Prints the transcript of `line`, which starts `onerom hardware sign`, with
/// the signing key table `table`. `server` signs where it's given and the
/// test key file otherwise. `typed` is what the user types. With `--json` the
/// values and the question go to stderr and the terminal shows them before
/// the JSON.
async fn sign_run(line: &str, typed: &str, table: &SignerTable, server: Option<&Server>) {
    // A word longer than 40 characters is shortened.
    let shown: Vec<String> = line
        .split_whitespace()
        .map(|word| {
            if word.len() > 40 {
                format!("<{} characters>", word.len())
            } else {
                shell_word(word)
            }
        })
        .collect();
    println!("$ {}", shown.join(" "));
    let (_dir, key) = key_file();
    let (args, options) = sign_args_of(line, &key);
    let mut screen = Screen::default();
    let signing = if server.is_some() {
        // The fake server stands in for the one --signer identifies.
        None
    } else {
        match key_source(
            (args.signer.as_deref(), args.key_id),
            args.key.as_deref(),
            args.pin.as_deref(),
            &mut screen.clone(),
        ) {
            Ok(signing) => Some(signing),
            Err(e) => return failed(e),
        }
    };
    let signer = match &signing {
        Some(signing) => signer_for(table, signing, &args.manufacturer).await,
        // A signing server's key is found by its ID and checked against the
        // manufacturer before its public key is fetched.
        None => signer_with_id(table, args.key_id.unwrap_or_default()).and_then(|signer| {
            check_key_allows(signer, &args.manufacturer)?;
            Ok(signer)
        }),
    };
    let signer = match signer {
        Ok(signer) => signer,
        Err(e) => {
            print!("{}", screen.take());
            return failed(e);
        }
    };
    let mut keyboard = Keyboard::new(typed, &screen);
    let mut out = screen.clone();
    let result = match (server, &signing) {
        (Some(server), _) => {
            sign_values(
                &args,
                server,
                (signer, table),
                &options,
                &mut screen,
                &mut keyboard,
                &mut out,
            )
            .await
        }
        (None, Some(signing)) => {
            sign_values(
                &args,
                signing,
                (signer, table),
                &options,
                &mut screen,
                &mut keyboard,
                &mut out,
            )
            .await
        }
        (None, None) => unreachable!(),
    };
    print!("{}", screen.take());
    if let Err(e) = result {
        failed(e);
    }
}

/// A key file signing a fire-24-f by piers.rocks on 20260101.
const KEY_FILE_LINE: &str = "onerom hardware sign --chip-id DE3F9C232F655B6B --board fire-24-f --manufacturer piers.rocks --date 20260101 --key key.pem";

/// Key 1 on [`SIGNER`] signing an L fire-40-a by onerom.org on 20260101, with
/// `--pin`.
fn server_line() -> String {
    format!(
        "onerom hardware sign --chip-id DE3F9C232F655B6B --board fire-40-a --size L --manufacturer onerom.org --date 20260101 --signer {SIGNER} --key-id 1 --pin 1234"
    )
}

#[test]
fn help() {
    super::help(&["onerom", "hardware", "sign", "--help"]);
}

/// Command lines refused before anything is signed.
#[test]
fn refused_command_lines() {
    let signature = signature_hex("fire-24-f", "piers.rocks", "20260101", 1);
    let sign = "onerom hardware sign --chip-id DE3F9C232F655B6B --board fire-24-f --manufacturer onerom.org";
    refused_lines(&[
        "onerom hardware sign --board fire-24-f --manufacturer onerom.org --key key.pem"
            .to_string(),
        "onerom hardware sign --chip-id DE3F9C232F655B6 --board fire-24-f --manufacturer onerom.org --key key.pem".to_string(),
        "onerom hardware sign --chip-id DE3F9C232F655B6X --board fire-24-f --manufacturer onerom.org --key key.pem".to_string(),
        sign.to_string(),
        format!("{sign} --signer https://HOST"),
        format!("{sign} --signer http://HOST --key-id 2"),
        format!("{sign} --key key.pem --key-id 2"),
        format!("{sign} --signer https://HOST --key-id 2 --key key.pem"),
        format!("{sign} --key key.pem --dry-run"),
        format!("{sign} --key key.pem --size L"),
        "onerom hardware sign --chip-id DE3F9C232F655B6B --board fire-40-a --manufacturer onerom.org --key key.pem".to_string(),
        format!("{sign} --key key.pem --date 20991231"),
        format!("{sign} --key key.pem --force"),
        format!("{sign} --key-id 2 --signature {signature}"),
    ]);
}

/// A key file, answered y, then n. Then with `--yes`, with `--json` and with a
/// manufacturer a shell would change.
#[tokio::test]
async fn with_a_key_file() {
    let table = table(None);
    println!("### answered y");
    sign_run(KEY_FILE_LINE, "y\n", &table, None).await;
    println!();
    println!("### answered n");
    sign_run(KEY_FILE_LINE, "n\n", &table, None).await;
    println!();
    println!("### with --yes");
    sign_run(&format!("{KEY_FILE_LINE} --yes"), "", &table, None).await;
    println!();
    println!("### with --json, answered y");
    sign_run(&format!("{KEY_FILE_LINE} --json"), "y\n", &table, None).await;
    println!();
    println!("### a manufacturer a shell would change");
    let line = KEY_FILE_LINE.replace("piers.rocks", "Piers's&Co");
    sign_run(&line, "y\n", &table, None).await;
}

/// Acme Retro's key file, which is encrypted with a PIN, signing an M
/// fire-40-b by Acme Retro. Answered y.
#[tokio::test]
async fn with_a_manufacturers_encrypted_key_file() {
    let words = [
        "onerom",
        "hardware",
        "sign",
        "--chip-id",
        "DE3F9C232F655B6B",
        "--board",
        "fire-40-b",
        "--size",
        "M",
        "--manufacturer",
        "Acme Retro",
        "--key",
        "acme.pem",
    ];
    println!("$ {}", shell_line(&words));
    let signing = acme_signing();
    let (args, options) = args_of(&words);
    let table = table(None);
    let signer = signer_for(&table, &signing, &args.manufacturer)
        .await
        .unwrap();
    let mut screen = Screen::default();
    let mut keyboard = Keyboard::new("y\n", &screen);
    let mut out = screen.clone();
    let result = sign_values(
        &args,
        &signing,
        (signer, &table),
        &options,
        &mut screen,
        &mut keyboard,
        &mut out,
    )
    .await;
    print!("{}", screen.take());
    if let Err(e) = result {
        failed(e);
    }
}

/// A signing server holding the test key. The PIN asked for at the terminal,
/// then with `--pin` answered y and n. Then with `--dry-run`, without and with
/// `--json`, with `-b` and `--yes`, and with `--json`.
#[tokio::test]
async fn with_a_signing_server() {
    let table = table(None);
    let line = server_line();
    let server = Server::test_key();

    println!("### the PIN asked for at the terminal");
    println!("$ {}", line.replace(" --pin 1234", ""));
    pin_prompt("signing key #1");
    println!("~ (continues as below)");
    println!();
    println!("### with --pin, answered y");
    sign_run(&line, "y\n", &table, Some(&server)).await;
    println!();
    println!("### with --pin, answered n");
    sign_run(&line, "n\n", &table, Some(&server)).await;
    println!();
    println!("### with --dry-run");
    sign_run(&format!("{line} --dry-run"), "", &table, Some(&server)).await;
    println!();
    println!("### with --dry-run and --json");
    sign_run(
        &format!("{line} --dry-run --json"),
        "",
        &table,
        Some(&server),
    )
    .await;
    println!();
    println!("### with -b and --yes");
    let short = format!("{} --yes", line.replace("--board", "-b"));
    sign_run(&short, "", &table, Some(&server)).await;
    println!();
    println!("### with --json, answered y");
    sign_run(&format!("{line} --json"), "y\n", &table, Some(&server)).await;
}

/// Keys and signatures refused. A key file the table doesn't contain, a key ID
/// it doesn't contain, a retired key and a server whose key 1 isn't the
/// table's. Then a dry run signed with another key, a recorded signature
/// other than the one checked and a manufacturer too long to fit in OTP.
#[tokio::test]
async fn refused() {
    let table = table(None);
    let line = server_line();

    println!("### a key file the table doesn't contain");
    {
        use ed25519_dalek::pkcs8::EncodePrivateKey as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.pem");
        let pem = ed25519_dalek::SigningKey::from_bytes(&[2; 32])
            .to_pkcs8_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
            .unwrap();
        std::fs::write(&path, pem.as_bytes()).unwrap();
        println!("$ {}", KEY_FILE_LINE.replace("key.pem", "other.pem"));
        let other = KeyFile::read(&path, None).unwrap();
        match signing_key(&table, &other.public_key()) {
            Ok(_) => println!("~ accepted"),
            Err(e) => failed(e),
        }
    }
    println!();
    println!("### key ID 7, which the table doesn't contain");
    let id_7 = line.replace("--key-id 1", "--key-id 7");
    sign_run(&id_7, "y\n", &table, Some(&Server::test_key())).await;
    println!();
    println!("### key 1 retired");
    let retired = crate::test_board::table(Some(serde_json::json!({})));
    sign_run(&line, "y\n", &retired, Some(&Server::test_key())).await;
    println!();
    println!("### the server's key 1 isn't the table's key 1 (written from the code)");
    println!("$ {line}");
    let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32])
        .verifying_key()
        .to_bytes();
    written_failure(&check_server_key(&table, 1, &other, SIGNER).unwrap_err());
    println!();
    println!("### the server's dry run is signed with another key");
    sign_run(&line, "y\n", &table, Some(&Server::another_key())).await;
    println!();
    println!("### the server records another signature from the one checked");
    let records_another = Server {
        records_another: true,
        ..Server::test_key()
    };
    sign_run(&line, "y\n", &table, Some(&records_another)).await;
    println!();
    println!("### a manufacturer too long to fit in OTP");
    let long = KEY_FILE_LINE.replace("piers.rocks", &"x".repeat(2100));
    sign_run(&long, "y\n", &table, None).await;
}

/// A manufacturer key 1 doesn't allow, with a key file and then a signing
/// server. Key 1 may sign only onerom.org.
#[tokio::test]
async fn a_manufacturer_the_key_doesnt_allow() {
    let table = table_allowing(&["onerom.org"]);
    println!("### a key file");
    sign_run(KEY_FILE_LINE, "y\n", &table, None).await;
    println!();
    println!("### a signing server");
    let line = server_line().replace("onerom.org", "piers.rocks");
    sign_run(&line, "y\n", &table, Some(&Server::test_key())).await;
}
