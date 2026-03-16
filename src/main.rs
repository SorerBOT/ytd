use std::process::Command;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;

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

    let mut file = tokio::fs::File::create("test_file.mp4").await?;

    let mut byte_stream = response.bytes_stream();

    while let Some(item) = byte_stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>
{
    let url = "https://www.youtube.com/watch?v=NV401gLmiAk";

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
