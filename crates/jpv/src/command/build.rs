use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use flate2::read::GzDecoder;
use lib::database::{self, Input};
use lib::Dirs;
use reqwest::Method;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::config::{Config, DownloadOverrides, IndexKind};
use crate::Args;

const USER_AGENT: &str = concat!("jpv/", env!("CARGO_PKG_VERSION"));

#[derive(Parser)]
pub(crate) struct BuildArgs {
    /// Path to load JMDICT file from. By default this will be download into a local cache directory.
    #[arg(long, value_name = "path")]
    jmdict_path: Option<PathBuf>,
    /// Path to load kanjidic2 file from. By default this will be download into a local cache directory.
    #[arg(long, value_name = "path")]
    kanjidic2_path: Option<PathBuf>,
    /// Path to load jmnedict file from. By default this will be download into a local cache directory.
    #[arg(long, value_name = "path")]
    jmnedict_path: Option<PathBuf>,
    /// Force a dictionary rebuild.
    #[arg(long, short = 'f')]
    force: bool,
}

pub(crate) async fn run(
    _: &Args,
    build_args: &BuildArgs,
    dirs: &Dirs,
    config: &Config,
) -> Result<()> {
    let overrides = DownloadOverrides {
        jmdict_path: build_args.jmdict_path.as_deref(),
        kanjidic2_path: build_args.kanjidic2_path.as_deref(),
        jmnedict_path: build_args.jmnedict_path.as_deref(),
    };

    let to_download = config.to_download(dirs, overrides);

    let mut futures: Vec<Pin<Box<dyn Future<Output = Result<()>>>>> = Vec::new();

    for download in &to_download {
        ensure_parent_dir(&download.index_path).await;

        // SAFETY: We are the only ones calling this function now.
        let result = lib::data::open(&download.index_path);

        match result {
            Ok(data) => match database::Index::open(data) {
                Ok(..) => {
                    if !build_args.force {
                        tracing::info!(
                            "Dictionary already exists at {}",
                            download.index_path.display()
                        );
                        continue;
                    } else {
                        tracing::info!(
                            "Dictionary already exists at {} (forcing rebuild)",
                            download.index_path.display()
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "Rebuilding since exists, but could not open: {error}: {}",
                        download.index_path.display()
                    );
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                bail!(e)
            }
        }

        futures.push(Box::pin(async {
            let (path, data) = read_or_download(
                download.path.as_deref(),
                dirs,
                &download.url_name,
                &download.url,
            )
            .await
            .context("loading JMDICT")?;

            tracing::info!("Loading `{}` from {}", download.name, path.display());

            let input = match download.kind {
                IndexKind::Jmdict => Input::Jmdict(&data[..]),
                IndexKind::Kanjidic2 => Input::Kanjidic(&data[..]),
                IndexKind::Jmnedict => Input::Jmnedict(&data[..]),
            };

            let start = Instant::now();
            let data = database::build(&download.name, input)?;
            let duration = Instant::now().duration_since(start);

            fs::write(&download.index_path, data.as_slice())
                .await
                .with_context(|| anyhow!("{}", download.index_path.display()))?;

            tracing::info!(
                "Took {duration:?} to build index at {}",
                download.index_path.display()
            );
            Ok(())
        }));
    }

    for future in futures {
        future.await?;
    }

    crate::dbus::shutdown().await?;
    Ok(())
}

async fn read_or_download(
    path: Option<&Path>,
    dirs: &Dirs,
    name: &str,
    url: &str,
) -> Result<(PathBuf, String), anyhow::Error> {
    let (path, bytes) = match path {
        Some(path) => (path.to_owned(), fs::read(path).await?),
        None => {
            let path = dirs.cache_dir(name);

            let bytes = if !path.is_file() {
                download(url, &path)
                    .await
                    .with_context(|| anyhow!("Downloading {url} to {}", path.display()))?
            } else {
                fs::read(&path).await?
            };

            (path, bytes)
        }
    };

    let mut input = GzDecoder::new(&bytes[..]);
    let mut string = String::new();
    input
        .read_to_string(&mut string)
        .with_context(|| path.display().to_string())?;
    Ok((path, string))
}

async fn download(url: &str, path: &Path) -> Result<Vec<u8>> {
    tracing::info!("Downloading {url} to {}", path.display());

    ensure_parent_dir(path).await;

    let client = reqwest::ClientBuilder::new().build()?;

    let request = client
        .request(Method::GET, url)
        .header("User-Agent", USER_AGENT)
        .build()?;

    let mut response = client.execute(request).await?;

    let mut f = File::create(path).await?;
    let mut data = Vec::new();

    while let Some(chunk) = response.chunk().await? {
        f.write_all(chunk.as_ref()).await?;
        data.extend_from_slice(chunk.as_ref());
    }

    Ok(data)
}

async fn ensure_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        let is_dir = match fs::metadata(parent).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Ok(metadata) if !metadata.is_dir() => false,
            _ => true,
        };

        if !is_dir {
            let _ = fs::create_dir_all(parent).await;
        }
    }
}
