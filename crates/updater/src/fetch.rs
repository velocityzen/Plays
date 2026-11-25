//! Fetching over HTTPS: the feed, read as it comes, and downloads into a
//! file, no bigger than the feed says, hashed as they come. HTTP only to
//! this machine, for tests; a redirect from HTTPS stays HTTPS.

use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};
use url::{Host, Url};

use crate::feed::File;
use crate::hash::{self, Hashing};
use crate::{Error, Result};

const CONNECT: Duration = Duration::from_secs(15);
const RESPONSE: Duration = Duration::from_secs(30);
/// The longest a download may go without a byte coming.
const READ: Duration = Duration::from_secs(60);

pub(crate) struct Client {
    https: Agent,
    /// For HTTP to this machine.
    local: Agent,
}

impl Client {
    /// Saying it's `user_agent`, such as `Plays/0.1.0`.
    pub fn new(user_agent: &str) -> Self {
        Self {
            https: agent(true, user_agent),
            local: agent(false, user_agent),
        }
    }

    /// What `url` answers, to read as it comes; dropped, it stops coming.
    pub fn open(&self, url: &Url) -> Result<impl BufRead + use<>> {
        Ok(BufReader::new(self.get(url)?))
    }

    /// `file`, from `url`, into `to`, as the feed says it is; `progress`
    /// told how many bytes have come. Kept if it's there already.
    pub fn download(
        &self,
        url: &Url,
        file: &File,
        to: &Path,
        progress: &mut dyn FnMut(u64),
        cancel: &AtomicBool,
    ) -> Result<()> {
        if to.exists() && hash::of_file(to).is_ok_and(|sha| hash::same(&sha, &file.sha256)) {
            progress(file.size);
            return Ok(());
        }
        let partial = partial(to);
        let downloaded = self.download_into(url, file, &partial, progress, cancel);
        if let Err(err) = downloaded {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
        fs::rename(&partial, to)?;
        Ok(())
    }

    fn download_into(
        &self,
        url: &Url,
        file: &File,
        partial: &Path,
        progress: &mut dyn FnMut(u64),
        cancel: &AtomicBool,
    ) -> Result<()> {
        let mut body = self.get(url)?;
        let mut out = Hashing::new(BufWriter::new(fs::File::create(partial)?));
        let mut buffer = vec![0; 64 << 10];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let read = body
                .read(&mut buffer)
                .map_err(|err| read_error(url, err, file.size))?;
            if read == 0 {
                break;
            }
            if out.written() + read as u64 > file.size {
                return Err(Error::TooBig {
                    url: url.to_string(),
                    limit: file.size,
                });
            }
            out.write_all(&buffer[..read])?;
            progress(out.written());
        }
        let written = out.written();
        let (sha, writer) = out.finish();
        let file_out = writer
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        file_out.sync_all()?;
        if written != file.size || !hash::same(&sha, &file.sha256) {
            return Err(Error::Mismatch(url.to_string()));
        }
        Ok(())
    }

    /// The body `url` answers with, if it answers 200.
    fn get(&self, url: &Url) -> Result<impl Read + use<>> {
        let agent = self.agent(url)?;
        let response = agent
            .get(url.as_str())
            .call()
            .map_err(|err| Error::Network {
                url: url.to_string(),
                message: err.to_string(),
            })?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::Http {
                url: url.to_string(),
                status,
            });
        }
        Ok(response.into_body().into_reader())
    }

    fn agent(&self, url: &Url) -> Result<&Agent> {
        match url.scheme() {
            "https" => Ok(&self.https),
            "http" if is_local(url) => Ok(&self.local),
            _ => Err(Error::Insecure(url.to_string())),
        }
    }
}

fn agent(https_only: bool, user_agent: &str) -> Agent {
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    Agent::config_builder()
        .https_only(https_only)
        .http_status_as_error(false)
        .user_agent(user_agent)
        .timeout_connect(Some(CONNECT))
        .timeout_recv_response(Some(RESPONSE))
        .timeout_per_read(Some(READ))
        .tls_config(tls)
        .build()
        .into()
}

/// Whether `url` is to this machine.
pub(crate) fn is_local(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Where a download goes until it has all come and been checked.
fn partial(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_owned();
    name.push(".partial");
    to.with_file_name(name)
}

fn read_error(url: &Url, err: io::Error, limit: u64) -> Error {
    if err.kind() == io::ErrorKind::FileTooLarge {
        return Error::TooBig {
            url: url.to_string(),
            limit,
        };
    }
    Error::Network {
        url: url.to_string(),
        message: err.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/fetch.rs"]
mod tests;
