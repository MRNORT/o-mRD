use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading { progress: f32 },
    Completed,
    Failed(String),
    AlreadyExists,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DownloadItem {
    pub beatmapset_id: u64,
    pub title: String,
    pub artist: String,
    pub status: DownloadStatus,
    pub file_path: Option<PathBuf>,
}

pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("osu-mania-dl/0.1")
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build downloader client");
        Self { client }
    }

    /// Downloads a beatmapset .osz file using Nerinyan mirror.
    /// Falls back to other mirrors if the primary fails.
    pub async fn download(
        &self,
        beatmapset_id: u64,
        output_dir: &PathBuf,
        prefer_no_video: bool,
        progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
    ) -> Result<PathBuf> {
        let filename = format!("{}.osz", beatmapset_id);
        let output_path = output_dir.join(&filename);

        // Already downloaded?
        if output_path.exists() {
            let _ = progress_tx
                .send(DownloadProgress {
                    beatmapset_id,
                    status: DownloadStatus::AlreadyExists,
                    file_path: Some(output_path.clone()),
                })
                .await;
            return Ok(output_path);
        }

        // Try mirrors in order
        let mut mirrors = vec![
            format!("https://api.nerinyan.moe/d/{}", beatmapset_id),
            format!("https://beatconnect.io/b/{}", beatmapset_id),
            format!("https://api.chimu.moe/v1/download/{}", beatmapset_id),
        ];

        if prefer_no_video {
            mirrors[0] = format!("https://api.nerinyan.moe/d/{}?novideo=1", beatmapset_id);
            mirrors[1] = format!("https://beatconnect.io/b/{}?novideo=1", beatmapset_id);
        }

        let mut last_err = anyhow!("No mirrors available");

        for mirror_url in &mirrors {
            match self
                .try_download(beatmapset_id, mirror_url, &output_path, &progress_tx)
                .await
            {
                Ok(path) => return Ok(path),
                Err(e) => {
                    log::warn!("Mirror {} failed: {}", mirror_url, e);
                    last_err = e;
                    // Remove partial file if it exists
                    let _ = tokio::fs::remove_file(&output_path).await;
                }
            }
        }

        let _ = progress_tx
            .send(DownloadProgress {
                beatmapset_id,
                status: DownloadStatus::Failed(last_err.to_string()),
                file_path: None,
            })
            .await;

        Err(last_err)
    }

    async fn try_download(
        &self,
        beatmapset_id: u64,
        url: &str,
        output_path: &PathBuf,
        progress_tx: &tokio::sync::mpsc::Sender<DownloadProgress>,
    ) -> Result<PathBuf> {
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("HTTP {}", resp.status()));
        }

        let content_length = resp.content_length();

        // Create parent dir
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = File::create(output_path).await?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;

        let _ = progress_tx
            .send(DownloadProgress {
                beatmapset_id,
                status: DownloadStatus::Downloading { progress: 0.0 },
                file_path: None,
            })
            .await;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;
            file.write_all(&chunk).await?;

            let progress = if let Some(total) = content_length {
                (downloaded as f32 / total as f32).clamp(0.0, 1.0)
            } else {
                -1.0 // unknown
            };

            let _ = progress_tx
                .send(DownloadProgress {
                    beatmapset_id,
                    status: DownloadStatus::Downloading { progress },
                    file_path: None,
                })
                .await;
        }

        file.flush().await?;
        drop(file);

        // Verify file is non-empty
        let metadata = tokio::fs::metadata(output_path).await?;
        if metadata.len() < 100 {
            tokio::fs::remove_file(output_path).await?;
            return Err(anyhow!("Downloaded file too small (likely error page)"));
        }

        let _ = progress_tx
            .send(DownloadProgress {
                beatmapset_id,
                status: DownloadStatus::Completed,
                file_path: Some(output_path.clone()),
            })
            .await;

        Ok(output_path.clone())
    }
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub beatmapset_id: u64,
    pub status: DownloadStatus,
    pub file_path: Option<PathBuf>,
}
