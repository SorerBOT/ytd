use std::process::Command;
use std::time::Duration;
use rand::RngExt;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Deserialize, Debug)]
struct HttpHeaders
{
    #[serde(rename = "User-Agent")]
    user_agent: String,

    #[serde(rename = "Accept")]
    accept: String,

    #[serde(rename = "Accept-Language")]
    accept_language: String,

    #[serde(rename = "Sec-Fetch-Mode")]
    sec_fetch_mode: String
}

#[derive(Deserialize, Debug)]
struct Video
{
    title: String,
    fulltitle: String,
    url: String,
    resolution: String,
    width: Option<u32>,
    height: Option<u32>,
    http_headers: HttpHeaders
}

fn get_video_from_url(url: &str) -> Result<Video,Box<dyn std::error::Error>>
{
    let output = Command::new("yt-dlp")
        .arg("--dump-json")
        .arg("-f")
        .arg("best[ext=mp4]")
        .arg(url)
        .output()?;

    let json_content = String::from_utf8(output.stdout)?;

    let content_serialized: Video = serde_json::from_str(&json_content)?;

    Ok(content_serialized)
}

async fn download_video(video: &Video) -> Result<(), Box<dyn std::error::Error>>
{
    let mut client_headers = HeaderMap::new();
    client_headers.append("User-Agent", HeaderValue::from_str(&video.http_headers.user_agent)?);
    client_headers.append("Accept", HeaderValue::from_str(&video.http_headers.accept)?);
    client_headers.append("Accept-Language", HeaderValue::from_str(&video.http_headers.accept_language)?);
    client_headers.append("Sec-Fetch-Mode", HeaderValue::from_str(&video.http_headers.sec_fetch_mode)?);

    let client = Client::builder()
        .default_headers(client_headers)
        .build()?;

    let response = client.get(&video.url).send().await?
        .error_for_status()?;

    let segment_index = response.text().await?;

    let segments_urls: Vec<String> = segment_index
        .lines()
        .filter(|line| !line.starts_with("#"))
        .map(|line| line.to_string())
        .collect();

    let segments_count = segments_urls.len();

    println!("Preparing to download {} segments from YouTube.", segments_count);

    let file_name = format!("{}.mp4", video.fulltitle).replace(" ", "_");
    let mut file = tokio::fs::File::create(&file_name).await?;

    let chunks_count = 3; // only 3 because youtube blocks me :(
    let chunk_size = segments_count / chunks_count;

    let mut handles = Vec::new();

    let downloaded_segments = Arc::new(AtomicUsize::new(0));

    for (chunk_number, chunk) in segments_urls.chunks(chunk_size).enumerate()
    {
        let worker_client = client.clone();
        let worker_urls = chunk.to_vec();
        let worker_counter = downloaded_segments.clone();

        let handle = tokio::spawn(async move
            {
                let file_name = format!("ytd_{}.tmp", chunk_number);
                let mut chunk_file = tokio::fs::File::create(&file_name).await.unwrap();

                for (segment_idx, segment_url) in worker_urls.iter().enumerate()
                {
                    if segment_idx % 10 == 0
                    {
                        let random_timeout = rand::rng().random_range(200..500);
                        tokio::time::sleep(tokio::time::Duration::from_millis(random_timeout)).await;
                        worker_counter.fetch_add(10, Ordering::Relaxed);
                    }

                    let segment_response = worker_client.get(segment_url).send().await.unwrap();
                    let mut byte_stream = segment_response.bytes_stream();

                    while let Some(item) = byte_stream.next().await
                    {
                        let segment_data = item.unwrap();
                        chunk_file.write_all(&segment_data).await.unwrap();
                    }
                }

                worker_counter.fetch_add(worker_urls.len() % 10, Ordering::Relaxed);
            });

        handles.push(handle);

        println!("Finished setting up worker number {}", chunk_number);
    }

    let progress_worker_count = downloaded_segments.clone();
    let progress_worker_handle = tokio::spawn(async move
        {
            loop
            {
                let previous_downloaded_segments_count = progress_worker_count.load(Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let current_downloaded_segments_count = progress_worker_count.load(Ordering::Relaxed);
                let progress_percentage = (current_downloaded_segments_count * 100) / segments_count;
                let download_speed = (current_downloaded_segments_count - previous_downloaded_segments_count) * 2;

                println!("Downloaded {}% of the video. Currently downloading {} MiB / S.", progress_percentage, download_speed);
            }
        });

    for handle in handles
    {
        handle.await.unwrap();
    }

    progress_worker_handle.abort();

    for temp_file_idx in 0..chunks_count
    {
        let file_name = format!("ytd_{}.tmp", temp_file_idx);
        let mut temp_file = tokio::fs::File::open(&file_name).await?;
        tokio::io::copy(&mut temp_file, &mut file).await?;
        tokio::fs::remove_file(&file_name).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>
{
    let url = "https://www.youtube.com/watch?v=REOZAvxdm4o";

    let video: Video = get_video_from_url(url)
        .expect("Failed to download video.");

    println!("Found video with title: {}", video.title);
    println!("Chosen format with resolution: {}", video.resolution);

    let result = download_video(&video).await;
    match result
    {
        Ok(()) =>
        {
            println!("Download complete.");
        },
        Err(err) =>
        {
            println!("Error downloading video: {}", err);
        }
    }

    Ok(())
}
