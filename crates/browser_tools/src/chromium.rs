use anyhow::Context as _;
use futures::{AsyncReadExt, AsyncSeekExt, AsyncWrite, io::BufReader};
use http_client::HttpClient;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// The Chrome for Testing build the browser tool provisions when no Chromium
/// is already present. Pinned so a given Zed build reproduces the same browser
/// behavior; the version's download URL and SHA-256 are read from Chrome for
/// Testing's own manifest at download time.
const PINNED_CHROMIUM_VERSION: &str = "153.0.8010.47";

const KNOWN_GOOD_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json";

/// Resolve a Chromium binary for launching a browser session.
///
/// Resolution order: the configured override, the `ZED_BROWSER_CHROMIUM_PATH`
/// environment variable, an installed Chrome/Edge, then a pinned Chrome for
/// Testing download cached under the user cache directory.
pub async fn resolve_chromium_binary(
    http_client: &Arc<dyn HttpClient>,
    override_path: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = discover_system_chromium() {
        return Ok(path);
    }
    ensure_pinned_chromium(http_client).await
}

fn discover_system_chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ZED_BROWSER_CHROMIUM_PATH") {
        return Some(PathBuf::from(path));
    }
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.exists() {
            return Some(path.to_path_buf());
        }
    }
    None
}

struct ChromiumPlatform {
    key: &'static str,
    archive_dir: &'static str,
    binary_rel: &'static str,
}

fn current_platform() -> anyhow::Result<ChromiumPlatform> {
    if cfg!(target_os = "windows") {
        Ok(ChromiumPlatform {
            key: "win64",
            archive_dir: "chrome-win64",
            binary_rel: "chrome.exe",
        })
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Ok(ChromiumPlatform {
                key: "mac-arm64",
                archive_dir: "chrome-mac-arm64",
                binary_rel: "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            })
        } else {
            Ok(ChromiumPlatform {
                key: "mac-x64",
                archive_dir: "chrome-mac-x64",
                binary_rel: "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            })
        }
    } else if cfg!(target_os = "linux") {
        Ok(ChromiumPlatform {
            key: "linux64",
            archive_dir: "chrome-linux64",
            binary_rel: "chrome",
        })
    } else {
        anyhow::bail!("automatic Chromium provisioning is not supported on this platform")
    }
}

async fn ensure_pinned_chromium(http_client: &Arc<dyn HttpClient>) -> anyhow::Result<PathBuf> {
    let platform = current_platform()?;
    let version_dir = chromium_cache_dir()?.join(PINNED_CHROMIUM_VERSION);
    let binary_path = version_dir.join(platform.archive_dir).join(platform.binary_rel);
    if binary_path.exists() {
        return Ok(binary_path);
    }

    let (url, sha256) = fetch_chromium_metadata(http_client, platform.key).await?;
    download_and_extract(http_client, &url, &sha256, &version_dir).await?;
    util::fs::make_file_executable(&binary_path)
        .await
        .with_context(|| format!("marking {binary_path:?} as executable"))?;
    Ok(binary_path)
}

fn chromium_cache_dir() -> anyhow::Result<PathBuf> {
    dirs::cache_dir()
        .map(|dir| dir.join("zed-browser"))
        .context("could not determine a cache directory for Chromium")
}

async fn fetch_chromium_metadata(
    http_client: &Arc<dyn HttpClient>,
    platform_key: &str,
) -> anyhow::Result<(String, String)> {
    let mut response = http_client
        .get(KNOWN_GOOD_VERSIONS_URL, Default::default(), true)
        .await
        .context("fetching Chrome for Testing versions")?;
    let mut bytes = Vec::new();
    let mut body = response.body_mut();
    body.read_to_end(&mut bytes)
        .await
        .context("reading Chrome for Testing versions")?;
    let versions: Value =
        serde_json::from_slice(&bytes).context("parsing Chrome for Testing versions")?;

    let version = versions
        .get("versions")
        .and_then(Value::as_array)
        .context("Chrome for Testing versions response has no `versions` array")?
        .iter()
        .find(|entry| entry.get("version").and_then(Value::as_str) == Some(PINNED_CHROMIUM_VERSION))
        .with_context(|| format!("Chrome for Testing {PINNED_CHROMIUM_VERSION} is not available"))?;

    let download = version
        .get("downloads")
        .and_then(|downloads| downloads.get("chrome"))
        .and_then(Value::as_array)
        .context("Chrome for Testing version has no `chrome` downloads")?
        .iter()
        .find(|entry| entry.get("platform").and_then(Value::as_str) == Some(platform_key))
        .with_context(|| {
            format!("Chrome for Testing has no {platform_key} build for {PINNED_CHROMIUM_VERSION}")
        })?;

    let url = download
        .get("url")
        .and_then(Value::as_str)
        .context("Chrome for Testing download has no `url`")?
        .to_string();
    let sha256 = download
        .get("sha256")
        .and_then(Value::as_str)
        .context("Chrome for Testing download has no `sha256`")?
        .to_string();

    Ok((url, sha256))
}

async fn download_and_extract(
    http_client: &Arc<dyn HttpClient>,
    url: &str,
    sha256: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    let destination_parent = destination
        .parent()
        .context("Chromium cache destination has no parent")?;
    async_fs::create_dir_all(destination_parent)
        .await
        .with_context(|| format!("creating Chromium cache directory {destination_parent:?}"))?;

    let staging = tempfile::Builder::new()
        .prefix(".tmp-zed-chromium-")
        .tempdir_in(destination_parent)
        .with_context(|| format!("creating Chromium staging directory in {destination_parent:?}"))?
        .keep();

    let result = extract_chromium_archive(http_client, url, sha256, &staging).await;
    if let Err(err) = result {
        let _ = async_fs::remove_dir_all(&staging).await;
        return Err(err);
    }
    if let Err(err) = finalize_download(&staging, destination).await {
        let _ = async_fs::remove_dir_all(&staging).await;
        return Err(err);
    }

    Ok(())
}

async fn extract_chromium_archive(
    http_client: &Arc<dyn HttpClient>,
    url: &str,
    expected_sha256: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    log::info!("downloading pinned Chromium from {url}");
    let mut response = http_client
        .get(url, Default::default(), true)
        .await
        .with_context(|| format!("downloading Chromium from {url}"))?;

    let temp_file = tempfile::NamedTempFile::new()
        .with_context(|| format!("creating a temporary file for {url}"))?;
    let (temp_file, _temp_guard) = temp_file.into_parts();
    let mut writer = HashingWriter {
        writer: async_fs::File::from(temp_file),
        hasher: Sha256::new(),
    };
    futures::io::copy(&mut BufReader::new(response.body_mut()), &mut writer)
        .await
        .with_context(|| format!("saving Chromium archive from {url}"))?;
    let digest = format!("{:x}", writer.hasher.finalize());

    anyhow::ensure!(
        digest.eq_ignore_ascii_case(expected_sha256),
        "Chromium archive SHA-256 mismatch for {url}. Expected {expected_sha256}, got {digest}"
    );

    writer
        .writer
        .seek(SeekFrom::Start(0))
        .await
        .with_context(|| format!("seeking Chromium archive for {url}"))?;
    util::archive::extract_zip(destination, &mut writer.writer)
        .await
        .with_context(|| format!("extracting Chromium archive into {destination:?}"))?;

    Ok(())
}

async fn finalize_download(staging_path: &Path, destination_path: &Path) -> anyhow::Result<()> {
    if let Err(err) = async_fs::remove_dir_all(destination_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::warn!("failed to remove existing Chromium destination {destination_path:?}: {err:?}");
        }
    }
    async_fs::rename(staging_path, destination_path)
        .await
        .with_context(|| format!("renaming {staging_path:?} to {destination_path:?}"))?;
    Ok(())
}

struct HashingWriter<W: AsyncWrite + Unpin> {
    writer: W,
    hasher: Sha256,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for HashingWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        match Pin::new(&mut self.writer).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self.hasher.update(&buf[..n]);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_close(cx)
    }
}
