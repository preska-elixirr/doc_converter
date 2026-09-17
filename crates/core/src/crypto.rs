//! `.age` encryption of any file and authenticated restore. Two recipient
//! types, as `age` and `rage` use them: a passphrase (scrypt) and X25519
//! public keys. The file header says which secret opens a file.

use crate::{commit, copy_cancel, message, output, write_bytes, Result, SecretString};
use age::secrecy::ExposeSecret;
use std::{fs::File, io::BufReader, path::Path, sync::atomic::AtomicBool};

pub use age::x25519::{Identity, Recipient};

/// Public keys one `.age` output can carry.
pub const MAX_RECIPIENTS: usize = 20;

/// Encrypts `source` with a passphrase.
pub fn encrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    seal(
        source,
        destination,
        age::Encryptor::with_user_passphrase(password),
        cancel,
    )
}

/// Encrypts `source` so that the holder of any one of `recipients` can open
/// it. No password is involved; the sender cannot open the result unless
/// their own key is among the recipients.
pub fn encrypt_for(
    source: &Path,
    destination: &Path,
    recipients: &[Recipient],
    cancel: &AtomicBool,
) -> Result<()> {
    if recipients.is_empty() {
        return Err(message("Enter at least one public key."));
    }
    if recipients.len() > MAX_RECIPIENTS {
        return Err(message("At most 20 public keys are allowed."));
    }
    let encryptor =
        age::Encryptor::with_recipients(recipients.iter().map(|r| r as &dyn age::Recipient))
            .map_err(message)?;
    seal(source, destination, encryptor, cancel)
}

fn seal(
    source: &Path,
    destination: &Path,
    encryptor: age::Encryptor,
    cancel: &AtomicBool,
) -> Result<()> {
    let input = File::open(source)?;
    let mut temp = output(destination)?;
    let mut writer = encryptor.wrap_output(&mut temp).map_err(message)?;
    copy_cancel(input, &mut writer, cancel)?;
    writer.finish().map_err(message)?;
    commit(temp, destination, cancel)
}

/// Restores an `.age` file. A password file needs `password`; a file
/// encrypted to a public key needs the matching `identity`. The header
/// decides, so the wrong kind of secret fails with a message that names
/// the kind the file needs.
pub fn decrypt(
    source: &Path,
    destination: &Path,
    password: Option<&SecretString>,
    identity: Option<&Identity>,
    cancel: &AtomicBool,
) -> Result<()> {
    let decryptor = age::Decryptor::new(BufReader::new(File::open(source)?))
        .map_err(|_| message("Not a supported age encrypted file."))?;
    let reader = if decryptor.is_scrypt() {
        let password = password.ok_or_else(|| {
            message("This file was encrypted with a password. Enter the password.")
        })?;
        let identity = age::scrypt::Identity::new(password.clone());
        decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|_| message("Incorrect password or damaged encrypted file."))?
    } else {
        let identity = identity.ok_or_else(|| {
            message("This file was encrypted to a public key. Enter the matching secret key.")
        })?;
        decryptor
            .decrypt(std::iter::once(identity as &dyn age::Identity))
            .map_err(|e| match e {
                age::DecryptError::NoMatchingKeys => {
                    message("The secret key does not match this file.")
                }
                _ => message("Damaged encrypted file."),
            })?
    };
    let mut temp = output(destination)?;
    copy_cancel(reader, &mut temp, cancel)?;
    commit(temp, destination, cancel)
}

/// Public keys from user text: one `age1…` key per line. Blank lines and
/// `#` comments are ignored, duplicates are dropped, errors name the line.
pub fn parse_recipients(text: &str) -> Result<Vec<Recipient>> {
    let mut recipients: Vec<Recipient> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.to_ascii_uppercase().starts_with("AGE-SECRET-KEY-") {
            return Err(message(format!(
                "Line {}: this is a secret key. Enter the public key that starts with age1.",
                index + 1
            )));
        }
        let recipient: Recipient = line.parse().map_err(|_| {
            message(format!(
                "Line {}: not an age public key. A public key starts with age1.",
                index + 1
            ))
        })?;
        if !recipients.contains(&recipient) {
            recipients.push(recipient);
        }
    }
    if recipients.is_empty() {
        return Err(message("Enter at least one public key."));
    }
    if recipients.len() > MAX_RECIPIENTS {
        return Err(message("At most 20 public keys are allowed."));
    }
    Ok(recipients)
}

/// A secret key from user text: the bare `AGE-SECRET-KEY-1…` line, the whole
/// key file, or that file flattened onto one line by a single-line input.
/// The key is found by its prefix wherever it sits; `#` comments are skipped.
pub fn parse_identity(text: &str) -> Result<Identity> {
    const PREFIX: &str = "AGE-SECRET-KEY-1";
    let upper = text.to_ascii_uppercase();
    if let Some(start) = upper.find(PREFIX) {
        let body = &upper[start + PREFIX.len()..];
        let end = body
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(body.len());
        return upper[start..start + PREFIX.len() + end]
            .parse()
            .map_err(|_| {
                message("Not an age secret key. A secret key starts with AGE-SECRET-KEY-1.")
            });
    }
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| message("Enter the secret key."))?;
    if line.starts_with("age1") {
        return Err(message(
            "This is a public key. Enter the secret key that starts with AGE-SECRET-KEY-1.",
        ));
    }
    Err(message(
        "Not an age secret key. A secret key starts with AGE-SECRET-KEY-1.",
    ))
}

/// Creates a new key pair and saves the secret key as a standard age key
/// file: a `# public key:` comment, then the `AGE-SECRET-KEY-1…` line.
/// Returns the public key. Never overwrites.
pub fn write_identity_file(destination: &Path, cancel: &AtomicBool) -> Result<String> {
    let identity = Identity::generate();
    let public = identity.to_public().to_string();
    let created: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    let text = SecretString::from(format!(
        "# created: {}\n# public key: {public}\n{}\n",
        created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        identity.to_string().expose_secret()
    ));
    write_bytes(destination, text.expose_secret().as_bytes(), cancel)?;
    Ok(public)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    fn failure<T>(result: Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn crypto_roundtrip_wrong_password_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let enc = dir.path().join("secret.age");
        let out = dir.path().join("restored");
        let bytes = b"Private document \0 with binary bytes \xff";
        std::fs::write(&src, bytes).unwrap();
        let cancel = AtomicBool::new(false);
        encrypt(&src, &enc, secret("a long test passphrase".into()), &cancel).unwrap();
        assert!(decrypt(&enc, &out, Some(&secret("wrong".into())), None, &cancel).is_err());
        assert!(!out.exists());
        decrypt(
            &enc,
            &out,
            Some(&secret("a long test passphrase".into())),
            None,
            &cancel,
        )
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert!(encrypt(&src, &enc, secret("another password".into()), &cancel).is_err());
        let mut broken = std::fs::read(&enc).unwrap();
        broken.truncate(broken.len() - 8);
        std::fs::write(&enc, broken).unwrap();
        let partial = dir.path().join("partial");
        assert!(decrypt(
            &enc,
            &partial,
            Some(&secret("a long test passphrase".into())),
            None,
            &cancel
        )
        .is_err());
        assert!(!partial.exists());
    }

    #[test]
    fn public_key_roundtrip_needs_the_matching_secret_key() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("contract.docx");
        let bytes = b"Contract \0 bytes \xff";
        std::fs::write(&src, bytes).unwrap();
        let cancel = AtomicBool::new(false);
        let alice = Identity::generate();
        let bob = Identity::generate();
        let text = format!(
            "# team\n{}\n\n  {}  \n{}\n",
            alice.to_public(),
            bob.to_public(),
            alice.to_public()
        );
        let recipients = parse_recipients(&text).unwrap();
        assert_eq!(recipients.len(), 2, "duplicates are dropped");
        let enc = dir.path().join("contract.docx.age");
        encrypt_for(&src, &enc, &recipients, &cancel).unwrap();

        // A password or an unrelated key cannot open it, and nothing is written.
        let out = dir.path().join("restored.docx");
        let password = secret("a long test passphrase".into());
        let err = decrypt(&enc, &out, Some(&password), None, &cancel).unwrap_err();
        assert!(err.to_string().contains("public key"), "{err}");
        let stranger = Identity::generate();
        let err = decrypt(&enc, &out, None, Some(&stranger), &cancel).unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
        assert!(!out.exists());

        // Either recipient restores the exact bytes; a password given as well is ignored.
        decrypt(&enc, &out, Some(&password), Some(&bob), &cancel).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        let again = dir.path().join("again.docx");
        decrypt(&enc, &again, None, Some(&alice), &cancel).unwrap();
        assert_eq!(std::fs::read(&again).unwrap(), bytes);

        // A password file asks for its password even when a key is offered.
        let with_password = dir.path().join("contract.pw.age");
        encrypt(&src, &with_password, password, &cancel).unwrap();
        let err = decrypt(
            &with_password,
            &dir.path().join("never"),
            None,
            Some(&alice),
            &cancel,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Enter the password"), "{err}");
        assert!(encrypt_for(&src, &dir.path().join("none.age"), &[], &cancel).is_err());

        // Truncated ciphertext is refused without partial output.
        let mut broken = std::fs::read(&enc).unwrap();
        broken.truncate(broken.len() - 8);
        std::fs::write(&enc, broken).unwrap();
        let partial = dir.path().join("partial.docx");
        assert!(decrypt(&enc, &partial, None, Some(&alice), &cancel).is_err());
        assert!(!partial.exists());
    }

    #[test]
    fn key_text_parsing_rejects_the_wrong_kind_of_key() {
        let identity = Identity::generate();
        let public = identity.to_public().to_string();
        let secret_line = identity.to_string();
        let err = parse_recipients(secret_line.expose_secret()).unwrap_err();
        assert!(
            err.to_string().starts_with("Line 1: this is a secret key"),
            "{err}"
        );
        let err = parse_recipients(&format!("{public}\nage1notakey")).unwrap_err();
        assert!(
            err.to_string().starts_with("Line 2: not an age public key"),
            "{err}"
        );
        assert!(parse_recipients("\n# only a comment\n").is_err());
        let many = (0..=MAX_RECIPIENTS)
            .map(|_| Identity::generate().to_public().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_recipients(&many)
            .unwrap_err()
            .to_string()
            .contains("At most"));

        let parsed = parse_identity(secret_line.expose_secret()).unwrap();
        assert_eq!(parsed.to_public().to_string(), public);
        let file = format!(
            "# created: now\r\n# public key: {public}\r\n{}\r\n",
            secret_line.expose_secret()
        );
        assert_eq!(
            parse_identity(&file).unwrap().to_public().to_string(),
            public
        );
        // A single-line input strips the line breaks of a pasted key file.
        let flat = file.replace("\r\n", "");
        assert!(flat.starts_with("# created"), "{flat}");
        assert_eq!(
            parse_identity(&flat).unwrap().to_public().to_string(),
            public
        );
        let lower = secret_line.expose_secret().to_ascii_lowercase();
        assert_eq!(
            parse_identity(&lower).unwrap().to_public().to_string(),
            public
        );
        assert!(parse_identity("AGE-SECRET-KEY-1TOOSHORT").is_err());
        assert!(failure(parse_identity(&public)).contains("public key"));
        assert!(failure(parse_identity("   \n")).contains("Enter the secret key"));
    }

    #[test]
    fn identity_file_is_standard_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("age-secret-key.txt");
        let cancel = AtomicBool::new(false);
        let public = write_identity_file(&path, &cancel).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# created: "), "{text}");
        assert!(text.contains(&format!("\n# public key: {public}\n")));
        assert!(text.ends_with('\n'));
        assert_eq!(
            parse_identity(&text).unwrap().to_public().to_string(),
            public
        );
        assert!(write_identity_file(&path, &cancel).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
