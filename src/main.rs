use std::process::Command;
use rand::RngExt;
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

    let segment_index = response.text().await?;

    let segments_urls: Vec<String> = segment_index
        .lines()
        .filter(|line| !line.starts_with("#"))
        .map(|line| line.to_string())
        .collect();

    let segments_count = segments_urls.len();

    println!("Preparing to download {} segments from YouTube.", segments_count);

    let mut file = tokio::fs::File::create("test_file.mp4").await?;

    let chunks_count = 3; // only 3 because youtube blocks me :(
    let chunk_size = segments_count / chunks_count;

    let mut handles = Vec::new();

    for (chunk_number, chunk) in segments_urls.chunks(chunk_size).enumerate()
    {
        let worker_client = client.clone();
        let worker_urls = chunk.to_vec();

        let handle = tokio::spawn(async move
            {
                let file_name = format!("ytd_{}.tmp", chunk_number);
                let mut chunk_file = tokio::fs::File::create(&file_name).await.unwrap();

                for segment_url in worker_urls
                {
                    let random_timeout = rand::rng().random_range(200..600);
                    tokio::time::sleep(tokio::time::Duration::from_millis(random_timeout)).await;
                    let segment_response = worker_client.get(segment_url).send().await.unwrap();
                    let mut byte_stream = segment_response.bytes_stream();

                    while let Some(item) = byte_stream.next().await
                    {
                        let segment_data = item.unwrap();
                        chunk_file.write_all(&segment_data).await.unwrap();
                    }
                }
            });

        handles.push(handle);

        println!("Finished setting up worker number {}", chunk_number);
    }

    for handle in handles
    {
        handle.await.unwrap();
    }

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
