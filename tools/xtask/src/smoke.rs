use crate::{Workspace, output, run};
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    extension_startups: Arc<AtomicUsize>,
}

impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let extension_startups = Arc::new(AtomicUsize::new(0));
        let startups = extension_startups.clone();
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for connection in listener.incoming() {
                let stream = connection?;
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let startups = startups.clone();
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
                if path == "/extension-background" {
                    startups.fetch_add(1, Ordering::AcqRel);
                }
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
            extension_startups,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BrowserMode {
    Standard,
    InheritedIncognito,
    ManagedIncognito,
    ManagedStandard,
    ManagedSafeMode,
}

struct Session<'a> {
    driver: &'a Driver,
    id: String,
    browser_pid: u32,
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
        let home = log
            .parent()
            .context("Missing synthetic driver directory")?
            .join("home");
        fs::create_dir(&home)?;
        let file = File::create(log)?;
        let process = Command::new(workspace.output().join("chromedriver"))
            .env("HOME", home)
            .env_remove("GOOGLE_API_KEY")
            .env_remove("GOOGLE_DEFAULT_CLIENT_ID")
            .env_remove("GOOGLE_DEFAULT_CLIENT_SECRET")
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
        mode: BrowserMode,
    ) -> Result<Session<'_>> {
        self.start_session_with_extension(app, profile, downloads, mode, None)
    }

    fn start_session_with_extension(
        &self,
        app: &Path,
        profile: &Path,
        downloads: &Path,
        mode: BrowserMode,
        extension: Option<&Path>,
    ) -> Result<Session<'_>> {
        let mut args = vec![
            "--headless=new".to_string(),
            "--use-mock-keychain".into(),
            "--lang=en-US".into(),
            "--window-size=1200,900".into(),
            format!(
                "--log-net-log={}",
                profile.with_extension("netlog.json").display()
            ),
            format!("--user-data-dir={}", profile.display()),
        ];
        if matches!(
            mode,
            BrowserMode::InheritedIncognito | BrowserMode::ManagedIncognito
        ) {
            args.push("--incognito".into());
        }
        if mode == BrowserMode::ManagedIncognito {
            args.extend([
                "--stainless-managed-mode=incognito".into(),
                "--disable-extensions".into(),
                "--no-first-run".into(),
            ]);
        }
        if matches!(
            mode,
            BrowserMode::ManagedStandard | BrowserMode::ManagedSafeMode
        ) {
            args.push("--stainless-managed-mode=standard".into());
        }
        if mode == BrowserMode::ManagedSafeMode {
            args.push("--stainless-startup-safe-mode".into());
        }
        let mut preferences = json!({
            "download.default_directory": downloads,
            "download.prompt_for_download": false,
            "incognito.mode_availability": 0,
            "credentials_enable_service": true
        });
        if let Some(extension) = extension {
            args.push(format!("--load-extension={}", extension.display()));
            preferences["session.restore_on_startup"] = json!(1);
            preferences["profile.name"] = json!("Synthetic startup identity");
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
                        "excludeSwitches": [
                            "disable-background-networking",
                            "disable-client-side-phishing-detection",
                            "disable-default-apps",
                            "disable-sync",
                            "disable-features",
                            "disable-extensions",
                            "no-first-run"
                        ],
                        "prefs": preferences
                    }
                }}
            }),
        )?;
        let id = response["sessionId"]
            .as_str()
            .map(str::to_owned)
            .context("ChromeDriver returned no session ID")?;
        let browser_pid = response["capabilities"]["goog:processID"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0);
        let Some(browser_pid) = browser_pid else {
            self.stop_session(&id)?;
            anyhow::bail!("ChromeDriver returned no owned browser PID");
        };
        Ok(Session {
            driver: self,
            id,
            browser_pid,
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

fn google_service_requests(log: &Value) -> Result<Vec<String>> {
    let events = log["events"]
        .as_array()
        .context("NetLog has no event array")?;
    ensure!(!events.is_empty(), "NetLog recorded no events");
    let mut requests = Vec::new();
    for event in events {
        let Some(raw) = event["params"]["url"].as_str() else {
            continue;
        };
        let url = reqwest::Url::parse(raw).context("NetLog contains an invalid request URL")?;
        let Some(host) = url.host_str() else {
            continue;
        };
        if [
            "google.com",
            "dns.google",
            "googleapis.com",
            "googleusercontent.com",
            "gstatic.com",
            "gvt1.com",
            "gvt2.com",
            "ggpht.com",
            "googlevideo.com",
            "googlesource.com",
            "doubleclick.net",
            "googlesyndication.com",
            "google-analytics.com",
            "googletagmanager.com",
            "googletagservices.com",
            "chromeenterprise.google",
        ]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        {
            requests.push(raw.to_owned());
        }
    }
    Ok(requests)
}

fn check_network_log(profile: &Path) -> Result<()> {
    let path = profile.with_extension("netlog.json");
    let log: Value = serde_json::from_slice(
        &fs::read(&path).context("Browser did not write its diagnostic NetLog")?,
    )?;
    let requests = google_service_requests(&log)?;
    ensure!(
        requests.is_empty(),
        "Built-in Google requests remain: {requests:?}"
    );
    Ok(())
}

fn check_builtin_policies(driver: &Driver, session: &str) -> Result<Value> {
    driver.post(session, "/url", json!({"url": "chrome://policy/"}))?;
    let policies = driver.post(session, "/execute/async", json!({
        "script": r#"
            const done = arguments[arguments.length - 1];
            const deadline = Date.now() + 15000;
            function check() {
                const group = document.querySelector('policy-app')?.policyGroups_?.find(g => g.id === 'chrome');
                if (group) {
                    done(Object.fromEntries(group.policies.filter(p => p.source === 'stainless').map(p => [p.name, p])));
                } else if (Date.now() > deadline) {
                    done({error: 'Policy UI did not load'});
                } else {
                    setTimeout(check, 100);
                }
            }
            check();
        "#,
        "args": []
    }))?;
    for (name, value) in [
        ("BrowserSignin", json!(0)),
        ("SyncDisabled", json!(true)),
        ("TranslateEnabled", json!(false)),
        ("SearchSuggestEnabled", json!(false)),
        ("SpellCheckServiceEnabled", json!(false)),
        ("SafeBrowsingProtectionLevel", json!(0)),
        ("PasswordLeakDetectionEnabled", json!(false)),
        ("MetricsReportingEnabled", json!(false)),
        ("NetworkPredictionOptions", json!(2)),
        ("DefaultGeolocationSetting", json!(2)),
        ("EnableMediaRouter", json!(false)),
        ("BrowserGuestModeEnabled", json!(false)),
    ] {
        validate_builtin_policy(name, &policies[name], &value)?;
    }
    Ok(policies)
}

fn validate_builtin_policy(name: &str, policy: &Value, expected: &Value) -> Result<()> {
    // PolicyLevel::kMandatory and PolicyScope::kMachine in policy.mojom.
    const MANDATORY_LEVEL: u64 = 1;
    const MACHINE_SCOPE: u64 = 1;
    ensure!(
        policy.get("value") == Some(expected)
            && policy.get("source").and_then(Value::as_str) == Some("stainless")
            && policy.get("level").and_then(Value::as_u64) == Some(MANDATORY_LEVEL)
            && policy.get("scope").and_then(Value::as_u64) == Some(MACHINE_SCOPE),
        "Missing effective Stainless restriction {name} with value {expected}: {policy}"
    );
    Ok(())
}

fn check_component_delivery(driver: &Driver, session: &str) -> Result<()> {
    driver.post(session, "/url", json!({"url": "chrome://components/"}))?;
    let controls = driver.post(session, "/execute/async", json!({
        "script": r#"
            const done = arguments[arguments.length - 1];
            const deadline = Date.now() + 15000;
            function check() {
                const statuses = Array.from(document.querySelectorAll('[id^="status-"]'));
                if (statuses.length > 0) {
                    done({
                        id: statuses[0].id.slice('status-'.length),
                        removed: document.querySelectorAll('.button-check-update').length === 0,
                        unavailable: document.body.textContent.includes('Built-in component updates are unavailable in Stainless.')
                    });
                } else if (Date.now() > deadline) {
                    done({error: 'Component UI did not load'});
                } else {
                    setTimeout(check, 100);
                }
            }
            check();
        "#,
        "args": []
    }))?;
    ensure!(
        controls["removed"] == true && controls["unavailable"] == true,
        "Component delivery was not explicitly unavailable: {controls}"
    );
    let status = driver.post(session, "/execute/async", json!({
        "script": r#"
            const done = arguments[arguments.length - 1];
            chrome.send('checkUpdate', [arguments[0]]);
            import('chrome://resources/js/cr.js')
                .then(module => module.sendWithPromise('requestComponentsData'))
                .then(data => done(data.components.length > 0 && data.components.every(component =>
                    component.status.includes('Built-in component updates are unavailable in Stainless.'))),
                    error => done({error: String(error)}));
        "#,
        "args": [controls["id"]]
    }))?;
    ensure!(
        status == true,
        "Forced component update did not remain unavailable: {status}"
    );
    Ok(())
}

fn check_settings_ui(driver: &Driver, session: &str) -> Result<()> {
    driver.post(session, "/url", json!({"url": "chrome://settings/people"}))?;
    let result = driver.post(
        session,
        "/execute/async",
        json!({
            "script": include_str!("../../../tests/fixtures/settings_ui.js"),
            "args": []
        }),
    )?;
    ensure!(
        result["removed_settings_search"] == true
            && result["retained_local_settings"] == true
            && result["repeatable_import_ui_with_synthetic_backend"] == true,
        "Settings routing/search/local-function checks failed: {result}"
    );
    driver.post(session, "/url", json!({"url": "chrome://extensions/"}))?;
    let extensions = driver.post(
        session,
        "/execute/async",
        json!({
            "script": r#"
            const done = arguments[arguments.length - 1];
            const deadline = Date.now() + 15000;
            async function check() {
                const toolbar = document.querySelector('extensions-manager')
                    ?.shadowRoot?.querySelector('extensions-toolbar');
                if (toolbar?.shadowRoot?.querySelector('#loadUnpacked')) {
                    try {
                        await chrome.developerPrivate.autoUpdate();
                        done({error: 'Removed extension updater reported success'});
                    } catch (error) {
                        done({
                            removed: !toolbar.shadowRoot.querySelector('#updateNow'),
                            localControls: !!toolbar.shadowRoot.querySelector('#packExtensions'),
                            backendDenied: String(error).includes('unavailable in Stainless')
                        });
                    }
                } else if (Date.now() >= deadline) {
                    done({error: 'Extension settings did not load'});
                } else {
                    setTimeout(check, 100);
                }
            }
            check();
        "#,
            "args": []
        }),
    )?;
    ensure!(
        extensions["removed"] == true
            && extensions["localControls"] == true
            && extensions["backendDenied"] == true,
        "Extension settings removal weakened local controls or backend denial: {extensions}"
    );
    Ok(())
}

fn check_managed_incognito(
    driver: &Driver,
    app: &Path,
    profile: &Path,
    downloads: &Path,
    fixture: &Fixture,
) -> Result<()> {
    let session = driver.start_session(app, profile, downloads, BrowserMode::ManagedIncognito)?;
    let policies = check_builtin_policies(driver, &session)?;
    for (name, value) in [
        ("IncognitoModeAvailability", json!(2)),
        ("BrowserAddPersonEnabled", json!(false)),
        ("PasswordManagerEnabled", json!(false)),
        ("BackgroundModeEnabled", json!(false)),
        ("ExtensionInstallBlocklist", json!(["*"])),
    ] {
        validate_builtin_policy(name, &policies[name], &value)?;
    }
    driver.post(&session, "/url", json!({"url": fixture.url("/normal")}))?;
    let cookies = driver.post(
        &session,
        "/execute/sync",
        json!({"script": "return document.cookie;", "args": []}),
    )?;
    ensure!(
        cookies == "",
        "Managed Incognito inherited Standard cookies"
    );
    session.close()?;
    check_network_log(profile)?;
    Ok(())
}

fn session_inventory(profile: &Path) -> Result<BTreeMap<std::ffi::OsString, String>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(profile.join("Default/Sessions"))? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "Unexpected synthetic session file"
        );
        files.insert(entry.file_name(), stainless_release::sha256(&entry.path())?);
    }
    ensure!(
        !files.is_empty(),
        "The normal browser saved no session fixture"
    );
    Ok(files)
}

fn extension_configuration(profile: &Path, id: &str) -> Result<Value> {
    let mut configuration = serde_json::Map::new();
    for name in ["Preferences", "Secure Preferences"] {
        let path = profile.join("Default").join(name);
        if !path.exists() {
            continue;
        }
        let prefs: Value = serde_json::from_slice(&fs::read(path)?)?;
        if let Some(extension) = prefs["extensions"]["settings"].get(id) {
            configuration.insert(name.into(), extension.clone());
        }
    }
    ensure!(
        !configuration.is_empty(),
        "The normal browser retained no extension configuration"
    );
    Ok(Value::Object(configuration))
}

fn startup_configuration(profile: &Path) -> Result<Value> {
    let prefs: Value = serde_json::from_slice(&fs::read(profile.join("Default/Preferences"))?)?;
    let local: Value = serde_json::from_slice(&fs::read(profile.join("Local State"))?)?;
    Ok(json!({
        "session": prefs["session"],
        "pinned_tabs": prefs["pinned_tabs"],
        "profile_name": prefs["profile"]["name"],
        "exit_type": prefs["profile"]["exit_type"],
        "background_mode": local["background_mode"],
    }))
}

fn select_restored_tab(driver: &Driver, session: &str, url: &str) -> Result<bool> {
    let handles = driver.request(
        reqwest::Method::GET,
        &format!("/session/{session}/window/handles"),
        json!({}),
    )?;
    for handle in handles
        .as_array()
        .context("Browser returned no window handles")?
    {
        driver.post(session, "/window", json!({"handle": handle}))?;
        let location = driver.post(
            session,
            "/execute/sync",
            json!({"script": "return location.href;", "args": []}),
        )?;
        if location == url {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wait_for_fixture_extension(driver: &Driver, session: &str) -> Result<()> {
    let active = driver.post(
        session,
        "/execute/async",
        json!({
            "script": r#"
            const done = arguments[arguments.length - 1];
            const deadline = Date.now() + 15000;
            function check() {
                if (document.documentElement.dataset.safeModeExtension === 'running') {
                    done(true);
                } else if (Date.now() >= deadline) {
                    done({error: 'Synthetic extension did not execute'});
                } else {
                    setTimeout(check, 50);
                }
            }
            check();
        "#,
            "args": []
        }),
    )?;
    ensure!(
        active == true,
        "Normal extension execution was not observed: {active}"
    );
    Ok(())
}

fn check_managed_safe_mode(
    driver: &Driver,
    app: &Path,
    profile: &Path,
    downloads: &Path,
    fixture: &Fixture,
) -> Result<()> {
    fs::create_dir(profile)?;
    let rejected = profile.with_extension("rejected");
    fs::create_dir(&rejected)?;
    let retained = rejected.join("retained");
    fs::write(&retained, b"synthetic failed-start data")?;
    let diagnostics = File::create(rejected.join("startup.log"))?;
    let mut invalid = ProcessGuard(
        Command::new(app.join("Contents/MacOS/Stainless"))
            .env(
                "HOME",
                profile
                    .parent()
                    .context("Missing synthetic HOME")?
                    .join("home"),
            )
            .args([
                "--headless=new",
                "--use-mock-keychain",
                "--stainless-managed-mode=incognito",
                "--stainless-startup-safe-mode",
            ])
            .arg(format!("--user-data-dir={}", rejected.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(diagnostics))
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let rejected_status = loop {
        if let Some(status) = invalid.0.try_wait()? {
            break status;
        }
        ensure!(
            Instant::now() < deadline,
            "The browser ignored an invalid Safe Mode startup request"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    ensure!(
        !rejected_status.success()
            && fs::read_to_string(rejected.join("startup.log"))?
                .contains("Safe Mode requires a managed Standard root")
            && fs::read(retained)? == b"synthetic failed-start data",
        "An unsupported actual-browser Safe Mode startup did not fail without deleting its data"
    );
    fs::write(
        profile.join("Local State"),
        serde_json::to_vec(&json!({"background_mode":{"enabled":true}}))?,
    )?;
    let extension = profile.with_extension("extension");
    fs::create_dir(&extension)?;
    fs::write(
        extension.join("manifest.json"),
        serde_json::to_vec(&json!({
            "manifest_version": 3,
            "name": "Synthetic Safe Mode extension",
            "version": "1.0",
            "host_permissions": ["http://127.0.0.1/*"],
            "content_scripts": [{
                "matches": ["http://127.0.0.1/*"],
                "js": ["content.js"],
                "run_at": "document_end"
            }],
            "background": {"service_worker":"background.js"}
        }))?,
    )?;
    fs::write(
        extension.join("content.js"),
        b"document.documentElement.dataset.safeModeExtension = 'running';\n",
    )?;
    fs::write(
        extension.join("background.js"),
        format!(
            "function started() {{ fetch({}).catch(error => console.error(error)); }}\nchrome.runtime.onStartup.addListener(started);\nchrome.runtime.onInstalled.addListener(started);\nstarted();\n",
            serde_json::to_string(&fixture.url("/extension-background"))?
        ),
    )?;
    let session = driver.start_session_with_extension(
        app,
        profile,
        downloads,
        BrowserMode::ManagedStandard,
        Some(&extension),
    )?;
    driver.post(
        &session,
        "/url",
        json!({"url":fixture.url("/extension-check")}),
    )?;
    wait_for_fixture_extension(driver, &session)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.extension_startups.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    ensure!(
        fixture.extension_startups.load(Ordering::Acquire) > 0,
        "Normal extension background startup did not reach the loopback fixture"
    );
    driver.post(&session, "/url", json!({"url":"chrome://extensions/"}))?;
    let id = driver.post(&session, "/execute/async", json!({
        "script": r#"
            const done = arguments[arguments.length - 1];
            chrome.developerPrivate.getExtensionsInfo({includeDisabled:true,includeTerminated:true})
                .then(items => done(items.find(item => item.name === 'Synthetic Safe Mode extension')?.id),
                    error => done({error:String(error)}));
        "#,
        "args": []
    }))?;
    let id = id
        .as_str()
        .filter(|id| id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte)))
        .context("The installed synthetic extension has no valid identity")?;
    let saved_url = fixture.url("/saved-session");
    driver.post(&session, "/url", json!({"url":saved_url}))?;
    driver.post(&session, "/execute/sync", json!({
        "script": "sessionStorage.setItem('synthetic-saved-session','retained'); document.cookie='safe_fixture=identity; path=/'; return true;",
        "args": []
    }))?;
    session.close()?;
    check_network_log(profile)?;
    let path = profile.join("Default/Preferences");
    let mut prefs: Value = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        prefs["profile"]["exit_type"] == "Normal",
        "The fixture browser must quit normally before testing automatic restoration"
    );
    prefs["pinned_tabs"] = json!([{"url":saved_url}]);
    fs::write(path, serde_json::to_vec(&prefs)?)?;
    let configuration = startup_configuration(profile)?;
    ensure!(
        configuration["session"]["restore_on_startup"] == 1
            && configuration["background_mode"]["enabled"] == true,
        "The saved fixture must request session restoration and background mode"
    );
    let extensions = extension_configuration(profile, id)?;
    let sessions = session_inventory(profile)?;
    let background_starts = fixture.extension_startups.load(Ordering::Acquire);

    let session = driver.start_session(app, profile, downloads, BrowserMode::ManagedSafeMode)?;
    let targets = driver.post(
        &session,
        "/goog/cdp/execute",
        json!({"cmd":"Target.getTargets","params":{}}),
    )?;
    ensure!(
        targets["targetInfos"]
            .as_array()
            .is_some_and(|targets| targets.iter().all(|target| target["url"] != saved_url)),
        "Safe Mode automatically opened saved or pinned tabs: {targets}"
    );
    stainless_profiles::app::require_managed_safe_mode(app)?;
    let policies = check_builtin_policies(driver, &session)?;
    validate_builtin_policy(
        "BackgroundModeEnabled",
        &policies["BackgroundModeEnabled"],
        &json!(false),
    )?;
    validate_builtin_policy("RestoreOnStartup", &policies["RestoreOnStartup"], &json!(5))?;
    driver.post(
        &session,
        "/url",
        json!({"url":fixture.url("/safe-browsing")}),
    )?;
    let browsing = driver.post(&session, "/execute/sync", json!({
        "script": "return {title: document.title, extension: document.documentElement.dataset.safeModeExtension ?? null, cookie: document.cookie};",
        "args": []
    }))?;
    ensure!(
        browsing["title"] == "Stainless synthetic fixture"
            && browsing["extension"].is_null()
            && browsing["cookie"] == "safe_fixture=identity",
        "Safe Mode did not retain ordinary Standard browsing/identity without extension execution: {browsing}"
    );
    let safe_targets = driver.post(
        &session,
        "/goog/cdp/execute",
        json!({"cmd":"Target.getTargets","params":{}}),
    )?;
    ensure!(
        safe_targets["targetInfos"]
            .as_array()
            .is_some_and(|targets| targets.iter().all(|target| !target["url"]
                .as_str()
                .is_some_and(|url| url.starts_with(&format!("chrome-extension://{id}/"))))),
        "Safe Mode started a saved extension target: {safe_targets}"
    );
    ensure!(
        safe_targets["targetInfos"]
            .as_array()
            .is_some_and(|targets| targets.iter().all(|target| target["url"] != saved_url)),
        "Safe Mode restored a saved tab after initial startup: {safe_targets}"
    );
    session.close()?;
    check_network_log(profile)?;
    ensure!(
        session_inventory(profile)? == sessions,
        "Safe Mode rewrote or rotated the saved session/tab/app journals"
    );
    ensure!(
        startup_configuration(profile)? == configuration
            && extension_configuration(profile, id)? == extensions,
        "Safe Mode permanently changed saved startup, pinned-tab, identity or extension preferences"
    );
    ensure!(
        fixture.extension_startups.load(Ordering::Acquire) == background_starts,
        "Safe Mode executed the saved extension's background worker"
    );

    let session = driver.start_session(app, profile, downloads, BrowserMode::ManagedStandard)?;
    ensure!(
        select_restored_tab(driver, &session, &saved_url)?,
        "Normal relaunch did not restore the original session after Safe Mode exit"
    );
    let retained = driver.post(&session, "/execute/sync", json!({
        "script": "return {session: sessionStorage.getItem('synthetic-saved-session'), cookie: document.cookie};",
        "args": []
    }))?;
    ensure!(
        retained["session"] == "retained" && retained["cookie"] == "safe_fixture=identity",
        "Normal relaunch lost the previous session's storage or identity: {retained}"
    );
    wait_for_fixture_extension(driver, &session)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.extension_startups.load(Ordering::Acquire) == background_starts
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    ensure!(
        fixture.extension_startups.load(Ordering::Acquire) > background_starts,
        "Normal relaunch did not re-enable the saved extension's background startup"
    );
    let policies = check_builtin_policies(driver, &session)?;
    ensure!(
        policies["BackgroundModeEnabled"].is_null() && policies["RestoreOnStartup"].is_null(),
        "Temporary Safe Mode policies survived normal relaunch"
    );
    session.close()?;
    check_network_log(profile)?;
    Ok(())
}

fn save_about_screenshot(driver: &Driver, session: &str, report: &Path) -> Result<()> {
    let screenshot = driver.post(
        session,
        "/goog/cdp/execute",
        json!({"cmd": "Page.captureScreenshot", "params": {"format": "png"}}),
    )?;
    let data = screenshot["data"]
        .as_str()
        .context("Browser returned no screenshot")?;
    let destination = report.with_file_name("stainless-about.png");
    let mut decoder = ProcessGuard(
        Command::new("/usr/bin/base64")
            .arg("-D")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(File::create(&destination)?))
            .spawn()?,
    );
    let mut input = decoder
        .0
        .stdin
        .take()
        .context("Screenshot decoder has no input")?;
    input.write_all(data.as_bytes())?;
    drop(input);
    ensure!(
        decoder.0.wait()?.success(),
        "Cannot decode the browser screenshot"
    );
    ensure!(
        fs::read(&destination)?.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Browser screenshot is not a PNG"
    );
    Ok(())
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
    let session = driver.start_session(app, &profile, &downloads, BrowserMode::Standard)?;
    driver.post(&session, "/url", json!({"url": "chrome://version/"}))?;
    let product = driver.post(
        &session,
        "/execute/sync",
        json!({
            "script": "return {name: document.querySelector('tr .label')?.textContent.trim(), text: document.body.innerText.slice(0, 400)};",
            "args": []
        }),
    )?;
    ensure!(
        product["name"] == "Stainless"
            && product["text"]
                .as_str()
                .is_some_and(|text| text.contains("Synctune Open")),
        "Version page still has inherited branding: {product}"
    );
    check_builtin_policies(&driver, &session)?;
    check_component_delivery(&driver, &session)?;
    check_settings_ui(&driver, &session)?;
    driver.post(&session, "/url", json!({"url": "chrome://settings/help"}))?;
    let about = driver.post(&session, "/execute/async", json!({
        "script": r#"
            const done = arguments[arguments.length - 1];
            const deadline = Date.now() + 15000;
            function roots(root) {
                return [root, ...Array.from(root.querySelectorAll('*')).filter(e => e.shadowRoot).flatMap(e => roots(e.shadowRoot))];
            }
            function check() {
                const all = roots(document);
                const page = all.map(r => r.querySelector('settings-about-page')).find(Boolean);
                const text = all.flatMap(r => Array.from(r.querySelectorAll('*'))
                    .filter(e => !['STYLE', 'SCRIPT'].includes(e.tagName) && e.children.length === 0)
                    .map(e => e.textContent.trim())).join('\n');
                if (page && page.getBoundingClientRect().height > 0 && text.includes('About Stainless')) {
                    const profile = all.map(r => r.querySelector('#people .menu-label')).find(Boolean);
                    done({
                        title: document.title,
                        inheritedTitle: text.includes('About Chromium'),
                        copyrightOwner: text.includes('Synctune Open'),
                        profileTitle: profile?.textContent.trim(),
                        aiPageVisible: all.some(r => r.querySelector('a#ai')?.getBoundingClientRect().height > 0)
                    });
                } else if (Date.now() > deadline) {
                    done({error: 'About Stainless did not load', title: document.title, excerpt: text.slice(0, 2000)});
                } else {
                    setTimeout(check, 100);
                }
            }
            check();
        "#,
        "args": []
    }))?;
    ensure!(
        about["title"] == "Settings - About Stainless"
            && about["inheritedTitle"] == false
            && about["copyrightOwner"] == true
            && about["profileTitle"] == "Profiles"
            && about["aiPageVisible"] == false,
        "About page still has inherited branding: {about}"
    );
    save_about_screenshot(&driver, &session, report)?;
    std::thread::sleep(Duration::from_secs(30));
    driver.post(&session, "/url", json!({"url": fixture.url("/normal")}))?;
    let found = driver.post(&session, "/execute/sync", json!({
        "script": "return document.title === 'Stainless synthetic fixture' && window.find('synthetic browsing fixture');",
        "args": []
    }))?;
    ensure!(
        found == true,
        "Navigation/find did not reach the synthetic fixture"
    );
    let cross_origin = format!("http://localhost:{}/cross-origin", fixture.address.port());
    let frame = driver.post(&session, "/execute/async", json!({
        "script": "const done = arguments[arguments.length - 1]; const frame = document.createElement('iframe'); frame.onload = () => done(true); frame.src = arguments[0]; document.body.append(frame);",
        "args": [cross_origin]
    }))?;
    ensure!(frame == true, "Cross-origin fixture did not load");
    let targets = driver.post(
        &session,
        "/goog/cdp/execute",
        json!({"cmd":"Target.getTargets","params":{}}),
    )?;
    ensure!(
        targets["targetInfos"]
            .as_array()
            .is_some_and(|targets| targets
                .iter()
                .any(|target| target["type"] == "iframe" && target["url"] == cross_origin)),
        "Cross-origin fixture was not placed in an out-of-process iframe"
    );
    let sandbox: Value = serde_json::from_str(&output(
        workspace
            .chromium_command("python-bin/python3")?
            .arg(
                workspace
                    .root
                    .join("tests/fixtures/inspect_browser_sandbox.py"),
            )
            .arg("--browser-pid")
            .arg(session.browser_pid.to_string())
            .arg("--profile")
            .arg(&profile),
    )?)?;
    ensure!(
        sandbox["native_sandboxed_renderers"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "No native renderer sandbox evidence was returned"
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
    check_network_log(&profile)?;

    let session =
        driver.start_session(app, &profile, &downloads, BrowserMode::InheritedIncognito)?;
    driver.post(&session, "/url", json!({"url":fixture.url("/private")}))?;
    let cookie = driver.post(
        &session,
        "/execute/sync",
        json!({"script":"return document.cookie;","args":[]}),
    )?;
    ensure!(cookie == "", "Incognito inherited normal-profile cookies");
    session.close()?;
    check_network_log(&profile)?;

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
    check_managed_incognito(
        &driver,
        app,
        &temporary.path().join("managed-incognito"),
        &downloads,
        &fixture,
    )?;
    check_managed_safe_mode(
        &driver,
        app,
        &temporary.path().join("managed-safe-mode"),
        &downloads,
        &fixture,
    )?;
    let session = driver.start_session(app, &profile, &downloads, BrowserMode::Standard)?;
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
    driver.post(&session, "/url", json!({"url": "https://example.com/"}))?;
    let trusted_https = driver.post(
        &session,
        "/execute/sync",
        json!({
            "script": "return window.isSecureContext && document.title === 'Example Domain';",
            "args": []
        }),
    )?;
    ensure!(
        trusted_https == true,
        "Normal HTTPS did not load the public IANA example fixture"
    );
    session.close()?;
    check_network_log(&profile)?;
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
            "trusted_https_example_domain":true,
            "stainless_version_about_and_builtin_policy_source":true,
            "google_component_delivery_unavailable":true,
            "google_service_settings_removed":true,
            "settings_search_and_local_controls":true,
            "repeatable_import_ui_with_synthetic_backend":true,
            "extension_update_control_removed_and_backend_denied":true,
            "managed_incognito_policies_and_fresh_cookie_state":true,
            "managed_safe_mode_extensions_background_and_session_suppression":true,
            "managed_safe_mode_saved_preferences_and_session_journals_retained":true,
            "managed_safe_mode_normal_relaunch_and_session_storage":true,
            "managed_safe_mode_invalid_browser_startup_rejected_without_data_loss":true,
            "google_service_url_requests":0,
            "first_run_idle_observation_seconds":30,
            "chromedriver_background_network_suppression_excluded":true,
            "native_renderer_sandbox":sandbox,
            "cross_origin_out_of_process_iframe":true,
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
    fn validates_numeric_stainless_policy_metadata() {
        let policy = json!({
            "value": false,
            "source": "stainless",
            "level": 1,
            "scope": 1
        });
        validate_builtin_policy("SyntheticRestriction", &policy, &json!(false)).unwrap();
    }

    #[test]
    fn rejects_ineffective_or_malformed_stainless_policy_metadata() {
        let policy = json!({
            "value": false,
            "source": "stainless",
            "level": 1,
            "scope": 1
        });
        for (field, value) in [
            ("value", json!(true)),
            ("value", Value::Null),
            ("source", json!("platform")),
            ("level", json!(0)),
            ("level", json!(2)),
            ("level", json!("mandatory")),
            ("scope", json!(0)),
            ("scope", json!(2)),
            ("scope", json!("machine")),
        ] {
            let mut invalid = policy.clone();
            invalid[field] = value;
            let error = validate_builtin_policy("SyntheticRestriction", &invalid, &json!(false))
                .unwrap_err();
            assert!(error.to_string().contains("SyntheticRestriction"));
        }
        for field in ["value", "source", "level", "scope"] {
            let mut invalid = policy.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(
                validate_builtin_policy("SyntheticRestriction", &invalid, &json!(false)).is_err()
            );
        }
        assert!(
            validate_builtin_policy("SyntheticRestriction", &Value::Null, &json!(false)).is_err()
        );
    }

    #[test]
    fn detects_google_service_requests_without_matching_lookalike_hosts() {
        let log = json!({"events": [
            {"params": {"url": "https://clients2.google.com/service"}},
            {"params": {"url": "https://translate.googleapis.com/service"}},
            {"params": {"url": "https://gstatic.com/asset"}},
            {"params": {"url": "https://google.com.example.invalid/"}},
            {"params": {"url": "https://notgstatic.com/"}},
            {"params": {"url": "http://127.0.0.1:8080/fixture"}}
        ]});
        assert_eq!(
            google_service_requests(&log).unwrap(),
            vec![
                "https://clients2.google.com/service",
                "https://translate.googleapis.com/service",
                "https://gstatic.com/asset"
            ]
        );
        assert!(google_service_requests(&json!({})).is_err());
    }

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
                browser_pid: 1,
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
