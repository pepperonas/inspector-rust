//! GitHub HTTP behind a small `Transport` trait: `Ureq` in the app, a fake in
//! the tests (no test ever touches the network). Tokens only travel in the
//! `Authorization` header and never appear in an error.

use super::{
    classify_status, next_link, parse_account, parse_device_code, parse_poll, parse_repo,
    parse_repos, Account, DeviceCode, PollResult, RateLimit, Repo, ERR_BAD_DATA, ERR_NETWORK,
    MAX_REPO_PAGES,
};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub const API: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    /// Lowercase header names.
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Response {
    pub fn header(&self, k: &str) -> Option<String> {
        self.headers.get(k).cloned()
    }

    pub fn rate(&self) -> Option<RateLimit> {
        super::rate_from_headers(|k| self.header(k))
    }
}

pub trait Transport {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response, String>;
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Response, String>;
}

/// The real transport (ureq, already a dependency).
pub struct Ureq;

fn collect(r: ureq::Response) -> Response {
    let headers = r
        .headers_names()
        .into_iter()
        .filter_map(|n| {
            r.header(&n)
                .map(|v| (n.to_ascii_lowercase(), v.to_string()))
        })
        .collect();
    let status = r.status();
    let body = r.into_string().unwrap_or_default();
    Response {
        status,
        headers,
        body,
    }
}

impl Transport for Ureq {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
        let mut req = ureq::get(url).timeout(TIMEOUT);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        match req.call() {
            Ok(r) => Ok(collect(r)),
            Err(ureq::Error::Status(_, r)) => Ok(collect(r)),
            Err(_) => Err(ERR_NETWORK.into()),
        }
    }

    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Response, String> {
        let req = ureq::post(url)
            .timeout(TIMEOUT)
            .set("Accept", "application/json")
            .set("User-Agent", "inspector-rust");
        match req.send_form(form) {
            Ok(r) => Ok(collect(r)),
            Err(ureq::Error::Status(_, r)) => Ok(collect(r)),
            Err(_) => Err(ERR_NETWORK.into()),
        }
    }
}

pub fn headers(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Accept", "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ("User-Agent", "inspector-rust".to_string()),
        ("Authorization", format!("Bearer {}", token.trim())),
    ]
}

/// What one call learned besides its payload.
#[derive(Debug, Clone, Default)]
pub struct CallInfo {
    pub rate: Option<RateLimit>,
    /// `x-oauth-scopes` — what the token may do (empty for fine-grained).
    pub scopes: Option<String>,
}

fn get_json(
    t: &dyn Transport,
    url: &str,
    token: &str,
    info: &mut CallInfo,
) -> Result<(Value, Response), String> {
    let r = t.get(url, &headers(token))?;
    if let Some(rate) = r.rate() {
        info.rate = Some(rate);
    }
    if let Some(s) = r.header("x-oauth-scopes") {
        info.scopes = Some(s);
    }
    if !(200..300).contains(&r.status) {
        return Err(classify_status(r.status, r.rate().map(|x| x.remaining)).into());
    }
    let v: Value = serde_json::from_str(&r.body).map_err(|_| ERR_BAD_DATA.to_string())?;
    Ok((v, r))
}

pub fn fetch_account(
    t: &dyn Transport,
    token: &str,
    info: &mut CallInfo,
) -> Result<Account, String> {
    let (v, _) = get_json(t, &format!("{API}/user"), token, info)?;
    parse_account(&v).ok_or_else(|| ERR_BAD_DATA.into())
}

/// Every repository the user OWNS (public + private as far as the token
/// allows), following `Link: rel="next"` up to `MAX_REPO_PAGES`.
pub fn fetch_repos(
    t: &dyn Transport,
    token: &str,
    info: &mut CallInfo,
) -> Result<Vec<Repo>, String> {
    let mut url = Some(format!(
        "{API}/user/repos?affiliation=owner&per_page=100&sort=full_name"
    ));
    let mut out = Vec::new();
    let mut pages = 0;
    while let Some(u) = url.take() {
        let (v, r) = get_json(t, &u, token, info)?;
        if !v.is_array() {
            return Err(ERR_BAD_DATA.into());
        }
        out.extend(parse_repos(&v));
        pages += 1;
        if pages >= MAX_REPO_PAGES {
            break;
        }
        url = r.header("link").and_then(|l| next_link(&l));
    }
    Ok(out)
}

/// One repository incl. subscribers ("watchers") — only for the detail view.
pub fn fetch_repo(
    t: &dyn Transport,
    token: &str,
    full_name: &str,
    info: &mut CallInfo,
) -> Result<Repo, String> {
    let (v, _) = get_json(t, &format!("{API}/repos/{full_name}"), token, info)?;
    parse_repo(&v).ok_or_else(|| ERR_BAD_DATA.into())
}

/// Start the device flow: the code the user types at github.com/login/device.
pub fn device_start(t: &dyn Transport, client_id: &str, scope: &str) -> Result<DeviceCode, String> {
    let r = t.post_form(
        "https://github.com/login/device/code",
        &[("client_id", client_id), ("scope", scope)],
    )?;
    let v: Value = serde_json::from_str(&r.body).map_err(|_| ERR_BAD_DATA.to_string())?;
    if let Some(e) = v["error"].as_str() {
        return Err(v["error_description"].as_str().unwrap_or(e).to_string());
    }
    parse_device_code(&v).ok_or_else(|| ERR_BAD_DATA.into())
}

pub fn device_poll(
    t: &dyn Transport,
    client_id: &str,
    device_code: &str,
    interval_s: i64,
) -> PollResult {
    let r = t.post_form(
        "https://github.com/login/oauth/access_token",
        &[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    );
    match r {
        Ok(r) => serde_json::from_str::<Value>(&r.body)
            .map(|v| parse_poll(&v, interval_s))
            .unwrap_or_else(|_| PollResult::Failed(ERR_BAD_DATA.into())),
        // A network hiccup is not the end of the flow — keep polling.
        Err(_) => PollResult::Pending,
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::cell::RefCell;

    /// One recorded request: URL and headers.
    pub type Call = (String, Vec<(String, String)>);

    /// Answers by URL; records every request (with headers) for assertions.
    #[derive(Default)]
    pub struct Fake {
        pub routes: HashMap<String, Response>,
        pub calls: RefCell<Vec<Call>>,
        pub offline: bool,
    }

    impl Fake {
        pub fn route(
            mut self,
            url: &str,
            status: u16,
            body: &str,
            headers: &[(&str, &str)],
        ) -> Self {
            self.routes.insert(
                url.to_string(),
                Response {
                    status,
                    body: body.to_string(),
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                },
            );
            self
        }
    }

    impl Transport for Fake {
        fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
            self.calls.borrow_mut().push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            ));
            if self.offline {
                return Err(ERR_NETWORK.into());
            }
            Ok(self.routes.get(url).cloned().unwrap_or(Response {
                status: 404,
                ..Default::default()
            }))
        }

        fn post_form(&self, url: &str, _form: &[(&str, &str)]) -> Result<Response, String> {
            self.get(url, &[])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;
    use crate::github::{ERR_FORBIDDEN, ERR_RATE_LIMITED, ERR_UNAUTHORIZED};

    fn repos_url(page: Option<u32>) -> String {
        let base = format!("{API}/user/repos?affiliation=owner&per_page=100&sort=full_name");
        match page {
            None => base,
            Some(p) => format!("{base}&page={p}"),
        }
    }

    fn page(ids: std::ops::Range<i64>) -> String {
        serde_json::to_string(
            &ids.map(
                |i| serde_json::json!({"id": i, "name": format!("r{i}"), "stargazers_count": i}),
            )
            .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn repositories_follow_every_page() {
        let link2 = format!(
            "<{}>; rel=\"next\", <{}>; rel=\"last\"",
            repos_url(Some(2)),
            repos_url(Some(3))
        );
        let link3 = format!("<{}>; rel=\"next\"", repos_url(Some(3)));
        let t = Fake::default()
            .route(&repos_url(None), 200, &page(0..100), &[("link", &link2)])
            .route(
                &repos_url(Some(2)),
                200,
                &page(100..200),
                &[("link", &link3)],
            )
            .route(
                &repos_url(Some(3)),
                200,
                &page(200..230),
                &[
                    ("x-ratelimit-limit", "5000"),
                    ("x-ratelimit-remaining", "4997"),
                    ("x-ratelimit-reset", "100"),
                ],
            );
        let mut info = CallInfo::default();
        let repos = fetch_repos(&t, "tok", &mut info).unwrap();
        assert_eq!(repos.len(), 230);
        assert_eq!(t.calls.borrow().len(), 3);
        assert_eq!(info.rate.unwrap().remaining, 4997);
    }

    #[test]
    fn the_token_goes_only_into_the_authorization_header() {
        let t = Fake::default().route(
            &format!("{API}/user"),
            200,
            r#"{"id":1,"login":"p"}"#,
            &[("x-oauth-scopes", "")],
        );
        let mut info = CallInfo::default();
        fetch_account(&t, " secret ", &mut info).unwrap();
        let calls = t.calls.borrow();
        let (url, hs) = &calls[0];
        assert!(!url.contains("secret"));
        assert!(hs.contains(&("Authorization".into(), "Bearer secret".into())));
        assert_eq!(info.scopes.as_deref(), Some(""));
    }

    #[test]
    fn errors_are_classified_and_never_carry_the_token() {
        let user = format!("{API}/user");
        let cases = [
            (401, "0", ERR_UNAUTHORIZED),
            (403, "0", ERR_RATE_LIMITED),
            (403, "50", ERR_FORBIDDEN),
        ];
        for (status, remaining, want) in cases {
            let t = Fake::default().route(
                &user,
                status,
                "{}",
                &[
                    ("x-ratelimit-limit", "60"),
                    ("x-ratelimit-remaining", remaining),
                    ("x-ratelimit-reset", "9"),
                ],
            );
            let mut info = CallInfo::default();
            let e = fetch_account(&t, "ghp_secret", &mut info).unwrap_err();
            assert_eq!(e, want);
            assert!(!e.contains("secret"));
            assert_eq!(
                info.rate.unwrap().remaining,
                remaining.parse::<i64>().unwrap(),
                "rate limit kept even on error"
            );
        }
        let offline = Fake {
            offline: true,
            ..Default::default()
        };
        assert_eq!(
            fetch_account(&offline, "t", &mut CallInfo::default()).unwrap_err(),
            ERR_NETWORK
        );
        let bad = Fake::default().route(&user, 200, "<html>", &[]);
        assert_eq!(
            fetch_account(&bad, "t", &mut CallInfo::default()).unwrap_err(),
            ERR_BAD_DATA
        );
    }

    #[test]
    fn device_flow_round_trip() {
        let t = Fake::default()
            .route("https://github.com/login/device/code", 200,
                r#"{"device_code":"d","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#, &[])
            .route("https://github.com/login/oauth/access_token", 200, r#"{"error":"authorization_pending"}"#, &[]);
        let d = device_start(&t, "Iv1.x", "").unwrap();
        assert_eq!(d.user_code, "ABCD-1234");
        assert_eq!(
            device_poll(&t, "Iv1.x", &d.device_code, 5),
            PollResult::Pending
        );
        let bad = Fake::default().route("https://github.com/login/device/code", 200,
            r#"{"error":"device_flow_disabled","error_description":"Device Flow must be explicitly enabled for this App"}"#, &[]);
        assert!(device_start(&bad, "Iv1.x", "")
            .unwrap_err()
            .contains("Device Flow"));
        let offline = Fake {
            offline: true,
            ..Default::default()
        };
        assert_eq!(
            device_poll(&offline, "x", "d", 5),
            PollResult::Pending,
            "a hiccup keeps polling"
        );
    }
}
