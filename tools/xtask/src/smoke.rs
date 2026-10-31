use crate::{Workspace, output, run};
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct ProcessGuard(Child);
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.0.kill() {
                    eprintln!("Cannot stop smoke helper {}: {error}", self.0.id());
                }
                if let Err(error) = self.0.wait() {
                    eprintln!("Cannot reap smoke helper: {error}");
                }
            }
            Err(error) => eprintln!("Cannot inspect smoke helper: {error}"),
        }
    }
}

struct Fixture {
    address: std::net::SocketAddr,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
}

impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for connection in listener.incoming() {
                let stream = connection?;
                if stop.load(Ordering::Acquire) {
                    break;
                }
                workers.push(std::thread::spawn(move || -> Result<()> {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                match reader.read_line(&mut request) {
                    Ok(0) => return Ok(()),
                    Ok(_) => {}
                    Err(error) if matches!(error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(()),
                    Err(error) => return Err(error.into()),
                }
                let mut header_bytes = request.len();
                loop {
                    let mut line = String::new();
                    let count = reader.read_line(&mut line)?;
                    header_bytes += count;
                    ensure!(header_bytes <= 16 * 1024, "Fixture request headers exceed limit");
                    if count == 0 || line == "\r\n" { break; }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (body, content_type, extra) = if path == "/download" {
                    (
                        "synthetic Stainless download\n",
                        "application/octet-stream",
                        "Content-Disposition: attachment; filename=\"fixture.txt\"\r\n",
                    )
                } else {
                    (
                        "<!doctype html><title>Stainless synthetic fixture</title><p id=\"fixture\">synthetic browsing fixture</p><a id=\"download\" href=\"/download\">Download</a>",
                        "text/html; charset=utf-8",
                        "",
                    )
                };
                write!(
                    reader.get_mut(),
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
                Ok(())
                }));
            }
            for worker in workers {
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("Fixture worker panicked"))??;
            }
            Ok(())
        });
        Ok(Self {
            address,
            stopping,
            thread: Some(thread),
        })
    }
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn shutdown(&mut self) -> Result<()> {
        if self.thread.is_none() {
            return Ok(());
        }
        self.stopping.store(true, Ordering::Release);
        TcpStream::connect_timeout(&self.address, Duration::from_secs(1))?;
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Smoke fixture thread panicked"))??;
        }
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Smoke fixture shutdown failed: {error:#}");
        }
    }
}

struct Driver {
    client: Client,
    endpoint: String,
    _process: ProcessGuard,
}

struct Session<'a> {
    driver: &'a Driver,
    id: String,
    closed: bool,
}

impl std::ops::Deref for Session<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.id
    }
}

impl Session<'_> {
    fn close(mut self) -> Result<()> {
        self.driver.stop_session(&self.id)?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if !self.closed
            && let Err(error) = self.driver.stop_session(&self.id)
        {
            eprintln!("Cannot close the owned synthetic browser session: {error:#}");
        }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        match self._process.0.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => {
                eprintln!("Cannot inspect the owned ChromeDriver before shutdown: {error}");
                return;
            }
        }
        let response = self
            .client
            .get(format!("{}/shutdown", self.endpoint))
            .timeout(Duration::from_secs(2))
            .send();
        match response {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => eprintln!("ChromeDriver shutdown returned HTTP {}", response.status()),
            Err(error) => eprintln!("Cannot request owned ChromeDriver shutdown: {error}"),
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self._process.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(error) => {
                    eprintln!("Cannot await ChromeDriver shutdown: {error}");
                    return;
                }
            }
        }
        eprintln!(
            "ChromeDriver did not exit gracefully; terminating only its owned child process."
        );
    }
}

impl Driver {
    fn start(workspace: &Workspace, log: &Path) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let file = File::create(log)?;
        let process = Command::new(workspace.output().join("chromedriver"))
            .arg(format!("--port={port}"))
            .arg("--allowed-ips=127.0.0.1")
            .stdout(Stdio::from(file.try_clone()?))
            .stderr(Stdio::from(file))
            .spawn()
            .context(
                "Build the matching Chromium chromedriver target before browser smoke checks",
            )?;
        let driver = Self {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(60))
                .build()?,
            endpoint: format!("http://127.0.0.1:{port}"),
            _process: ProcessGuard(process),
        };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            if driver
                .client
                .get(format!("{}/status", driver.endpoint))
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                return Ok(driver);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        anyhow::bail!(
            "ChromeDriver did not become ready; inspect {}",
            log.display()
        )
    }

    fn request(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .request(method, format!("{}{path}", self.endpoint))
            .json(&body)
            .send()?;
        let status = response.status();
        let result: Value = response.json()?;
        ensure!(
            status.is_success(),
            "Browser smoke command {path} failed: {result}"
        );
        Ok(result["value"].clone())
    }

    fn post(&self, session: &str, path: &str, body: Value) -> Result<Value> {
        self.request(
            reqwest::Method::POST,
            &format!("/session/{session}{path}"),
            body,
        )
    }

    fn start_session(
        &self,
        app: &Path,
        profile: &Path,
        downloads: &Path,
        incognito: bool,
    ) -> Result<Session<'_>> {
        let mut args = vec![
            "--headless=new".to_string(),
            "--no-first-run".into(),
            "--use-mock-keychain".into(),
            format!("--user-data-dir={}", profile.display()),
        ];
        if incognito {
            args.push("--incognito".into());
        }
        let response = self.request(
            reqwest::Method::POST,
            "/session",
            json!({
                "capabilities": { "alwaysMatch": {
                    "browserName": "chrome",
                    "acceptInsecureCerts": false,
                    "goog:chromeOptions": {
                        "binary": app.join("Contents/MacOS/Stainless"),
                        "args": args,
                        "prefs": {
                            "download.default_directory": downloads,
                            "download.prompt_for_download": false
                        }
                    }
                }}
            }),
        )?;
        let id = response["sessionId"]
            .as_str()
            .map(str::to_owned)
            .context("ChromeDriver returned no session ID")?;
        Ok(Session {
            driver: self,
            id,
            closed: false,
        })
    }

    fn stop_session(&self, session: &str) -> Result<()> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/session/{session}"),
            json!({}),
        )?;
        Ok(())
    }
}

pub(super) fn check(workspace: &Workspace, app: &Path, report: &Path) -> Result<()> {
    ensure!(
        app.join("Contents/MacOS/Stainless").is_file(),
        "Expected the full Stainless.app"
    );
    let temporary = tempfile::tempdir()?;
    let profile = temporary.path().join("synthetic-profile");
    let downloads = temporary.path().join("downloads");
    fs::create_dir(&downloads)?;
    let mut fixture = Fixture::start()?;
    let driver = Driver::start(workspace, &temporary.path().join("chromedriver.log"))?;
    let session = driver.start_session(app, &profile, &downloads, false)?;
    driver.post(&session, "/url", json!({"url": fixture.url("/normal")}))?;
    let found = driver.post(&session, "/execute/sync", json!({
        "script": "return document.title === 'Stainless synthetic fixture' && window.find('synthetic browsing fixture');",
        "args": []
    }))?;
    ensure!(
        found == true,
        "Navigation/find did not reach the synthetic fixture"
    );
    let version = driver.post(
        &session,
        "/goog/cdp/execute",
        json!({"cmd":"Browser.getVersion","params":{}}),
    )?;
    ensure!(
        version["product"].as_str().is_some_and(|version| version
            .ends_with(workspace.manifest.chromium.version.as_deref().unwrap_or(""))),
        "Browser/Chromium version does not match the manifest"
    );
    driver.post(&session, "/goog/cdp/execute", json!({
        "cmd":"Browser.setDownloadBehavior", "params":{"behavior":"allow","downloadPath": downloads}
    }))?;
    driver.post(&session, "/execute/sync", json!({
        "script":"document.cookie='fixture=normal; path=/; Max-Age=3600'; document.querySelector('#download').click(); return true;",
        "args":[]
    }))?;
    let start = Instant::now();
    while !downloads.join("fixture.txt").exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(100));
    }
    ensure!(
        fs::read(downloads.join("fixture.txt"))? == b"synthetic Stainless download\n",
        "Downloaded fixture contents differ"
    );
    let tab = driver.post(&session, "/window/new", json!({"type":"tab"}))?;
    driver.post(&session, "/window", json!({"handle":tab["handle"]}))?;
    driver.post(&session, "/url", json!({"url":fixture.url("/second")}))?;
    let pdf = driver.post(
        &session,
        "/goog/cdp/execute",
        json!({"cmd":"Page.printToPDF","params":{}}),
    )?;
    ensure!(
        pdf["data"]
            .as_str()
            .is_some_and(|data| data.starts_with("JVBER")),
        "Print/PDF did not return a PDF"
    );
    session.close()?;

    let session = driver.start_session(app, &profile, &downloads, true)?;
    driver.post(&session, "/url", json!({"url":fixture.url("/private")}))?;
    let cookie = driver.post(
        &session,
        "/execute/sync",
        json!({"script":"return document.cookie;","args":[]}),
    )?;
    ensure!(cookie == "", "Incognito inherited normal-profile cookies");
    session.close()?;

    let history = profile.join("Default/History");
    let private = output(
        Command::new("/usr/bin/sqlite3")
            .arg(&history)
            .arg("SELECT count(*) FROM urls WHERE url LIKE '%/private';"),
    )?;
    ensure!(
        private.trim() == "0",
        "Incognito URL leaked into normal profile history"
    );
    let normal = output(
        Command::new("/usr/bin/sqlite3")
            .arg(&history)
            .arg("SELECT count(*) FROM urls WHERE url LIKE '%/normal';"),
    )?;
    ensure!(
        normal.trim() == "1",
        "Normal history did not persist across browser restarts"
    );

    run(Command::new("/usr/bin/openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-days",
            "1",
            "-keyout",
        ])
        .arg(temporary.path().join("key.pem"))
        .arg("-out")
        .arg(temporary.path().join("cert.pem")))?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let tls_port = listener.local_addr()?.port();
    drop(listener);
    let tls_log = File::create(temporary.path().join("tls-fixture.log"))?;
    let mut tls = ProcessGuard(
        workspace
            .chromium_command("python-bin/python3")?
            .arg(workspace.root.join("tests/fixtures/https_server.py"))
            .arg("--port")
            .arg(tls_port.to_string())
            .arg("--certificate")
            .arg(temporary.path().join("cert.pem"))
            .arg("--key")
            .arg(temporary.path().join("key.pem"))
            .stdout(Stdio::null())
            .stderr(Stdio::from(tls_log))
            .spawn()?,
    );
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", tls_port)).is_err()
        && start.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    ensure!(
        tls.0.try_wait()?.is_none() && TcpStream::connect(("127.0.0.1", tls_port)).is_ok(),
        "Synthetic TLS server did not start: {}",
        fs::read_to_string(temporary.path().join("tls-fixture.log"))?
    );
    let session = driver.start_session(app, &profile, &downloads, false)?;
    driver.post(&session, "/url", json!({"url":fixture.url("/normal")}))?;
    let restored_cookie = driver.post(
        &session,
        "/execute/sync",
        json!({"script":"return document.cookie;","args":[]}),
    )?;
    ensure!(
        restored_cookie == "fixture=normal",
        "Normal-profile cookie did not survive restart or Incognito isolation"
    );
    let response: Value = driver
        .client
        .post(format!("{}/session/{}/url", driver.endpoint, session.id))
        .json(&json!({"url":format!("https://localhost:{tls_port}/")}))
        .send()?
        .json()?;
    if response["value"]["error"] != "insecure certificate" {
        let interstitial = driver.post(
            &session,
            "/execute/sync",
            json!({
                "script": "return {title: document.title, errorCode: document.querySelector('#error-code')?.textContent, fixtureLoaded: document.body.textContent.includes('synthetic Stainless HTTPS fixture')};",
                "args": []
            }),
        )?;
        ensure!(
            interstitial["errorCode"].as_str().is_some_and(|code| code
                .trim()
                .eq_ignore_ascii_case("net::ERR_CERT_AUTHORITY_INVALID"))
                && interstitial["fixtureLoaded"] == false,
            "Self-signed TLS did not produce a certificate interstitial: {interstitial}; navigation: {response}"
        );
    }
    session.close()?;
    fixture.shutdown()?;
    fs::write(
        report,
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "chromium_revision":workspace.manifest.chromium.revision,
            "product_version":workspace.manifest.product.version,
            "synthetic_navigation_find_tabs":true,
            "download_contents":true,
            "devtools_protocol_and_pdf":true,
            "history_restart_and_incognito_cookie_history_isolation":true,
            "self_signed_tls_rejected":true,
            "clean_host_install":false,
            "installed_n_to_n_plus_one":false,
            "manual_native_browser_gates":"not_recorded"
        }))?,
    )?;
    println!("Synthetic full-browser smoke record: {}", report.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_the_browser_session_when_a_smoke_check_returns_early() {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || -> Result<String> {
            let deadline = Instant::now() + Duration::from_secs(3);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            reader.read_line(&mut request)?;
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header)?;
                if header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse()?;
                }
            }
            reader.read_exact(&mut vec![0; length])?;
            let body = "{\"value\":null}";
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(request)
        });
        let mut process = Command::new("/usr/bin/true").spawn().unwrap();
        process.wait().unwrap();
        let driver = Driver {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            endpoint: format!("http://{address}"),
            _process: ProcessGuard(process),
        };
        let result: Result<()> = (|| {
            let _session = Session {
                driver: &driver,
                id: "synthetic-session".into(),
                closed: false,
            };
            anyhow::bail!("synthetic smoke failure")
        })();
        assert!(result.is_err());
        assert!(
            server
                .join()
                .unwrap()
                .unwrap()
                .starts_with("DELETE /session/synthetic-session ")
        );
    }

    #[test]
    fn serves_only_synthetic_fixture_content() {
        let mut fixture = Fixture::start().unwrap();
        let response = Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(fixture.url("/"))
            .send()
            .unwrap()
            .text()
            .unwrap();
        assert!(response.contains("Stainless synthetic fixture"));
        fixture.shutdown().unwrap();
    }

    #[test]
    fn idle_preconnect_does_not_block_fixture_navigation() {
        let mut fixture = Fixture::start().unwrap();
        let idle = TcpStream::connect(fixture.address).unwrap();
        let response = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .get(fixture.url("/normal"))
            .send()
            .unwrap();
        assert!(response.status().is_success());
        drop(idle);
        fixture.shutdown().unwrap();
    }
}
