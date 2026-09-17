//! Password-based `.age` encryption of any file and authenticated restore.

use crate::{commit, copy_cancel, message, output, Result, SecretString};
use std::{fs::File, io::BufReader, path::Path, sync::atomic::AtomicBool};

pub fn encrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    let input = File::open(source)?;
    let mut temp = output(destination)?;
    let encryptor = age::Encryptor::with_user_passphrase(password);
    let mut writer = encryptor.wrap_output(&mut temp).map_err(message)?;
    copy_cancel(input, &mut writer, cancel)?;
    writer.finish().map_err(message)?;
    commit(temp, destination, cancel)
}

pub fn decrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    let decryptor = age::Decryptor::new(BufReader::new(File::open(source)?))
        .map_err(|_| message("Not a supported age encrypted file."))?;
    if !decryptor.is_scrypt() {
        return Err(message(
            "This build supports password-encrypted age files only.",
        ));
    }
    let identity = age::scrypt::Identity::new(password);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| message("Incorrect password or damaged encrypted file."))?;
    let mut temp = output(destination)?;
    copy_cancel(reader, &mut temp, cancel)?;
    commit(temp, destination, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

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
        assert!(decrypt(&enc, &out, secret("wrong".into()), &cancel).is_err());
        assert!(!out.exists());
        decrypt(&enc, &out, secret("a long test passphrase".into()), &cancel).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert!(encrypt(&src, &enc, secret("another password".into()), &cancel).is_err());
        let mut broken = std::fs::read(&enc).unwrap();
        broken.truncate(broken.len() - 8);
        std::fs::write(&enc, broken).unwrap();
        let partial = dir.path().join("partial");
        assert!(decrypt(
            &enc,
            &partial,
            secret("a long test passphrase".into()),
            &cancel
        )
        .is_err());
        assert!(!partial.exists());
    }
}
