//! Downloads one stream over HTTP in fixed-size ranges, reporting progress and stopping
//! promptly on Cancel. Small ranges keep progress accurate and let YouTube serve at full speed.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::EngineError;

/// Bytes per ranged request (10 MiB).
pub const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

/// Called with (bytes done, total bytes if known).
pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Downloads `url` into `dest` (created or truncated), `chunk_size` bytes per request, sending
/// `headers` with every request. Returns the number of bytes written. Leaves `dest` in place on
/// error; the job deletes it.
pub async fn download_ranged(
    client: &reqwest::Client,
    url: &str,
    headers: &HashMap<String, String>,
    chunk_size: u64,
    dest: &Path,
    cancel: &CancellationToken,
    progress: ProgressFn<'_>,
) -> Result<u64, EngineError> {
    let mut file = tokio::fs::File::create(dest).await?;
    let mut done: u64 = 0;
    let mut total: Option<u64> = None;
    let mut last_report = Instant::now();

    loop {
        let mut req = client.get(url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        req = req.header(RANGE, format!("bytes={}-{}", done, done + chunk_size - 1));

        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::Cancelled),
            r = req.send() => r.map_err(|e| EngineError::Http(e.without_url().to_string()))?,
        };
        let status = resp.status();
        if !status.is_success() {
            return Err(EngineError::Http(format!("HTTP {}", status.as_u16())));
        }

        // 200 means the server ignored the range and is sending the whole file.
        let whole_file = status == StatusCode::OK;
        if whole_file {
            if done > 0 {
                // Already wrote earlier chunks; start over (create truncates the file).
                file = tokio::fs::File::create(dest).await?;
                done = 0;
            }
            total = resp.content_length();
        } else if total.is_none() {
            total = content_range_total(&resp);
        }
        let expected = if whole_file {
            total
        } else {
            Some(match total {
                Some(t) => chunk_size.min(t.saturating_sub(done)),
                None => chunk_size,
            })
        };

        let mut got: u64 = 0;
        let mut body = resp.bytes_stream();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                n = body.next() => n,
            };
            match next {
                None => break,
                Some(Err(e)) => return Err(EngineError::Http(e.without_url().to_string())),
                Some(Ok(bytes)) => {
                    file.write_all(&bytes).await?;
                    got += bytes.len() as u64;
                    done += bytes.len() as u64;
                    if last_report.elapsed() >= PROGRESS_EVERY {
                        progress(done, total);
                        last_report = Instant::now();
                    }
                }
            }
        }

        let finished = match total {
            Some(t) => done >= t,
            // No total known: a chunk shorter than asked for is the last one.
            None => got < chunk_size,
        };
        if finished || whole_file {
            if whole_file && expected.is_some_and(|e| got < e) {
                return Err(EngineError::Http("connection ended early".into()));
            }
            break;
        }
        if expected.is_some_and(|e| got < e) || got == 0 {
            return Err(EngineError::Http("connection ended early".into()));
        }
    }

    file.flush().await?;
    progress(done, total);
    Ok(done)
}

/// Reads TOTAL from `Content-Range: bytes a-b/TOTAL`.
fn content_range_total(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// Fake stream server: answers `Range: bytes=a-b` with 206 and a `Content-Range` header.
    struct RangeResponder {
        body: Vec<u8>,
        /// Send fewer bytes than the range asks for (a dropped connection).
        truncate: bool,
        /// Wait this long before answering.
        delay: Option<Duration>,
    }

    impl Respond for RangeResponder {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let total = self.body.len() as u64;
            let range = req.headers.get("range").unwrap().to_str().unwrap();
            let (a, b) = range.trim_start_matches("bytes=").split_once('-').unwrap();
            let start: u64 = a.parse().unwrap();
            let end: u64 = b.parse::<u64>().unwrap().min(total - 1);
            let mut slice = self.body[start as usize..=end as usize].to_vec();
            if self.truncate {
                slice.truncate(slice.len() / 2);
            }
            let t = ResponseTemplate::new(206)
                .insert_header("Content-Range", format!("bytes {start}-{end}/{total}").as_str())
                .set_body_bytes(slice);
            match self.delay {
                Some(d) => t.set_delay(d),
                None => t,
            }
        }
    }

    fn body() -> Vec<u8> {
        (0u8..25).collect()
    }

    fn ua() -> HashMap<String, String> {
        HashMap::from([("User-Agent".to_string(), "UA".to_string())])
    }

    async fn server_with(responder: impl Respond + 'static) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/v")).respond_with(responder).mount(&server).await;
        server
    }

    fn no_progress(_: u64, _: Option<u64>) {}

    #[tokio::test]
    async fn downloads_in_ranges_and_reports_total() {
        let server = server_with(RangeResponder { body: body(), truncate: false, delay: None }).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.part");
        let calls = Mutex::new(Vec::new());
        let progress = |d: u64, t: Option<u64>| calls.lock().unwrap().push((d, t));

        let n = download_ranged(
            &reqwest::Client::new(),
            &format!("{}/v", server.uri()),
            &ua(),
            10,
            &dest,
            &CancellationToken::new(),
            &progress,
        )
        .await
        .unwrap();

        assert_eq!(n, 25);
        assert_eq!(std::fs::read(&dest).unwrap(), body());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|r| r.headers.get("user-agent").unwrap() == "UA"));
        assert_eq!(calls.lock().unwrap().last(), Some(&(25, Some(25))));
    }

    #[tokio::test]
    async fn http_403_is_error() {
        let server = server_with(ResponseTemplate::new(403)).await;
        let dir = tempfile::tempdir().unwrap();
        let r = download_ranged(
            &reqwest::Client::new(),
            &format!("{}/v", server.uri()),
            &ua(),
            10,
            &dir.path().join("out.part"),
            &CancellationToken::new(),
            &no_progress,
        )
        .await;
        assert!(matches!(r, Err(EngineError::Http(m)) if m.contains("403")));
    }

    #[tokio::test]
    async fn short_body_is_error() {
        let server = server_with(RangeResponder { body: body(), truncate: true, delay: None }).await;
        let dir = tempfile::tempdir().unwrap();
        let r = download_ranged(
            &reqwest::Client::new(),
            &format!("{}/v", server.uri()),
            &ua(),
            10,
            &dir.path().join("out.part"),
            &CancellationToken::new(),
            &no_progress,
        )
        .await;
        assert!(matches!(r, Err(EngineError::Http(_))));
    }

    #[tokio::test]
    async fn server_ignoring_range_still_works() {
        let server = server_with(ResponseTemplate::new(200).set_body_bytes(body())).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.part");
        let n = download_ranged(
            &reqwest::Client::new(),
            &format!("{}/v", server.uri()),
            &ua(),
            10,
            &dest,
            &CancellationToken::new(),
            &no_progress,
        )
        .await
        .unwrap();
        assert_eq!(n, 25);
        assert_eq!(std::fs::read(&dest).unwrap(), body());
    }

    #[tokio::test]
    async fn cancel_stops_download() {
        let server = server_with(RangeResponder {
            body: body(),
            truncate: false,
            delay: Some(Duration::from_secs(5)),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            c.cancel();
        });

        let started = Instant::now();
        let r = download_ranged(
            &reqwest::Client::new(),
            &format!("{}/v", server.uri()),
            &ua(),
            10,
            &dir.path().join("out.part"),
            &cancel,
            &no_progress,
        )
        .await;
        assert!(matches!(r, Err(EngineError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
