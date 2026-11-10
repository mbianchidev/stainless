use super::*;
use anyhow::bail;
use stainless_release::verify_asset;
use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

struct TestServer {
    process: Child,
    directory: tempfile::TempDir,
    port: u16,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        match self.process.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.process.kill() {
                    eprintln!("Cannot stop owned HTTPS update fixture: {error}");
                }
                if let Err(error) = self.process.wait() {
                    eprintln!("Cannot reap owned HTTPS update fixture: {error}");
                }
            }
            Err(error) => eprintln!("Cannot inspect owned HTTPS update fixture: {error}"),
        }
    }
}

impl TestServer {
    fn start(scenario: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join("certificate.conf"),
            "[req]\nprompt=no\ndistinguished_name=subject\n\
             [subject]\nCN=Stainless synthetic update fixture\n\
             [ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\n\
             [server]\nbasicConstraints=critical,CA:FALSE\n\
             keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n\
             subjectAltName=DNS:api.github.com,DNS:release-assets.githubusercontent.com\n",
        )?;
        for args in [
            vec![
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-config",
                "certificate.conf",
                "-extensions",
                "ca",
                "-keyout",
                "ca-key.pem",
                "-out",
                "ca.pem",
            ],
            vec![
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-config",
                "certificate.conf",
                "-keyout",
                "server-key.pem",
                "-out",
                "server.csr",
            ],
            vec![
                "x509",
                "-req",
                "-in",
                "server.csr",
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca-key.pem",
                "-CAcreateserial",
                "-days",
                "1",
                "-extfile",
                "certificate.conf",
                "-extensions",
                "server",
                "-out",
                "server.pem",
            ],
        ] {
            let output = Command::new("/usr/bin/openssl")
                .args(args)
                .current_dir(directory.path())
                .output()?;
            ensure!(
                output.status.success(),
                "Cannot generate synthetic HTTPS certificates: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let log = File::create(directory.path().join("server.log"))?;
        let process = Command::new("/usr/bin/python3")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github_https.py"))
            .arg("--scenario")
            .arg(scenario)
            .current_dir(directory.path())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()?;
        let mut server = Self {
            process,
            directory,
            port: 0,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = server.process.try_wait()? {
                bail!("HTTPS fixture exited ({status}): {}", server.log()?);
            }
            let ready = server.directory.path().join("port");
            if ready.is_file() {
                server.port = fs::read_to_string(ready)?.parse()?;
                TcpStream::connect(("127.0.0.1", server.port))?;
                return Ok(server);
            }
            ensure!(
                Instant::now() < deadline,
                "HTTPS fixture did not become ready: {}",
                server.log()?
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> Result<String> {
        Ok(fs::read_to_string(
            self.directory.path().join("server.log"),
        )?)
    }

    fn client(&self, trust_fixture: bool) -> Result<PreviewClient> {
        let mut builder = PreviewClient::client_builder()
            .proxy(reqwest::Proxy::https(format!(
                "http://127.0.0.1:{}",
                self.port
            ))?)
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10));
        if trust_fixture {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&fs::read(
                self.directory.path().join("ca.pem"),
            )?)?);
        }
        Ok(PreviewClient {
            client: builder.build()?,
            token: Zeroizing::new("synthetic-update-authorization".into()),
        })
    }
}

fn current() -> Product {
    Product {
        version: "0.1.0-preview.1".parse().unwrap(),
        build: 1,
        channel: "preview".into(),
        profile_schema: 1,
    }
}

#[test]
fn discovers_and_downloads_over_validated_https_without_forwarding_credentials() -> Result<()> {
    for scenario in ["download", "redirect"] {
        let server = TestServer::start(scenario)?;
        let client = server.client(true)?;
        let (feed, asset) = client
            .discover(&current(), "26.6")?
            .context("Synthetic newer preview was not discovered")?;
        assert_eq!(feed.release.build, 2);
        let download = server.directory.path().join("update.zip");
        client.download(&asset, &download)?;
        verify_asset(&download, &feed.asset)?;
        assert_eq!(fs::read(download)?, b"abc");
        assert!(client.discover(&feed.release, "26.6")?.is_none());
    }
    Ok(())
}

#[test]
fn failed_checks_are_errors_not_no_update_responses() -> Result<()> {
    for (scenario, expected) in [
        ("offline", "HTTPS request failed"),
        ("denied_401", "HTTP 401"),
        ("denied_403", "HTTP 403"),
        ("bad_json", "Malformed or incomplete"),
        ("bad_feed", "Malformed or incomplete"),
        ("wrong_target", "Wrong update target"),
        ("wrong_schema", "Profile schema migration"),
        ("stale_build", "Stale update or downgrade"),
        ("metadata_over_limit", "exceeds the size limit"),
    ] {
        let server = TestServer::start(scenario)?;
        let error = server
            .client(true)?
            .discover(&current(), "26.6")
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{scenario}: {message}");
        assert!(!message.contains("synthetic-update-authorization"));
        assert!(!server.directory.path().join("update.zip").exists());
    }
    Ok(())
}

#[test]
fn metadata_at_the_limit_is_accepted_but_an_untrusted_certificate_is_not() -> Result<()> {
    let server = TestServer::start("metadata_at_limit")?;
    assert!(server.client(true)?.discover(&current(), "26.6")?.is_some());
    let error = server
        .client(false)?
        .discover(&current(), "26.6")
        .unwrap_err();
    assert!(format!("{error:#}").contains("HTTPS request failed"));
    Ok(())
}

#[test]
fn interrupted_or_oversized_downloads_and_untrusted_redirects_fail() -> Result<()> {
    for (scenario, expected) in [
        ("truncated", "download interrupted"),
        ("oversized", "exceeded the declared size"),
        ("untrusted_redirect", "HTTPS request failed"),
        ("downgrade_redirect", "HTTPS request failed"),
        ("excessive_redirects", "HTTPS request failed"),
    ] {
        let server = TestServer::start(scenario)?;
        let client = server.client(true)?;
        let (_, asset) = client
            .discover(&current(), "26.6")?
            .context("Missing synthetic release")?;
        let download = server.directory.path().join("update.zip");
        let message = format!("{:#}", client.download(&asset, &download).unwrap_err());
        assert!(message.contains(expected), "{scenario}: {message}");
        if download.exists() {
            assert!(download.metadata()?.len() <= asset.size + 1);
        }
    }
    Ok(())
}

#[test]
fn downloaded_bad_hash_is_rejected_without_changing_the_app_or_profile() -> Result<()> {
    let server = TestServer::start("bad_hash")?;
    let client = server.client(true)?;
    let (feed, asset) = client
        .discover(&current(), "26.6")?
        .context("Missing synthetic release")?;
    let root = server.directory.path();
    let locations = crate::Locations {
        app: root.join("Stainless.app"),
        profile: root.join("synthetic-profile"),
        state: root.join("state"),
    };
    for path in [&locations.app, &locations.profile, &locations.state] {
        fs::create_dir(path)?;
    }
    fs::write(locations.app.join("sentinel"), b"synthetic installed app")?;
    fs::write(locations.profile.join("Bookmarks"), b"synthetic bookmarks")?;
    let download = root.join("update.zip");
    client.download(&asset, &download)?;
    let error = crate::install::stage(&locations, &feed, &download).unwrap_err();
    assert!(format!("{error:#}").contains("SHA-256 mismatch"));
    assert_eq!(
        fs::read(locations.app.join("sentinel"))?,
        b"synthetic installed app"
    );
    assert_eq!(
        fs::read(locations.profile.join("Bookmarks"))?,
        b"synthetic bookmarks"
    );
    assert!(!locations.state.join("ready.json").exists());
    Ok(())
}
