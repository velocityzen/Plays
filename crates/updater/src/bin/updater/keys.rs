//! The key pair a feed is signed with: made, and the secret key read with
//! its password.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use minisign::{KeyPair, PublicKey, SecretKey};
use updater::Keys;

use crate::{Result, at};

/// Where the password is taken from when it isn't asked for.
const PASSWORD: &str = "UPDATER_PASSWORD";

/// A secret key, and its public key.
pub(crate) struct Key {
    pub secret: SecretKey,
    pub public: PublicKey,
}

/// Make the key pair in `folder`: `updates.key`, encrypted with a
/// password, and `updates.pub`. Neither may be there already.
pub(crate) fn generate(folder: &Path) -> Result<()> {
    let secret = folder.join("updates.key");
    let public = folder.join("updates.pub");
    for path in [&secret, &public] {
        if path.exists() {
            return Err(format!("{} is there already", path.display()));
        }
    }
    let pair = KeyPair::generate_encrypted_keypair(password()).map_err(|err| err.to_string())?;
    fs::create_dir_all(folder).map_err(at(folder))?;
    let secret_box = pair
        .sk
        .to_box(Some("updater secret key"))
        .map_err(at(&secret))?;
    write_new(&secret, &secret_box.to_bytes(), true)?;
    let public_box = pair.pk.to_box().map_err(at(&public))?;
    write_new(&public, &public_box.to_bytes(), false)?;
    println!(
        "{}: the secret key; keep it, and its password, safe",
        secret.display()
    );
    println!(
        "{}: the public key, which the app checks feeds with:",
        public.display()
    );
    println!("{}", pair.pk.to_base64());
    Ok(())
}

/// The secret key at `path`, its password asked for.
pub(crate) fn read(path: &Path) -> Result<Key> {
    let secret = SecretKey::from_file(path, password()).map_err(at(path))?;
    let public = PublicKey::from_secret_key(&secret).map_err(at(path))?;
    Ok(Key { secret, public })
}

impl Key {
    /// minisign's signature of `text`, its trusted comment `app`.
    pub(crate) fn sign(&self, text: &str, app: &str) -> Result<String> {
        minisign::sign(
            Some(&self.public),
            &self.secret,
            Cursor::new(text),
            Some(app),
            None,
        )
        .map(minisign::SignatureBox::into_string)
        .map_err(|err| format!("can't sign it: {err}"))
    }

    /// Whether `signature`, as minisign writes it, is this key's.
    pub(crate) fn made(&self, signature: &str) -> Result<bool> {
        let signature = minisign::SignatureBox::from_string(signature)
            .map_err(|err| format!("a signature in the feed isn't one: {err}"))?;
        Ok(signature.keynum() == self.public.keynum())
    }

    /// Its public key, as the app has it.
    pub(crate) fn keys(&self) -> Result<Keys> {
        Keys::parse([self.public.to_base64().as_str()]).map_err(|err| err.to_string())
    }
}

fn password() -> Option<String> {
    std::env::var(PASSWORD).ok()
}

/// `bytes` written to `path`, which mustn't be there: readable by this
/// user alone, if it's `secret`.
fn write_new(path: &Path, bytes: &[u8], secret: bool) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = options.open(path).map_err(at(path))?;
    file.write_all(bytes).map_err(at(path))
}
