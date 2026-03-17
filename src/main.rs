use std::process::Command;
use std::time::Duration;
use rand::RngExt;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;
use tokio::sync::Semaphore;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::env;

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
    ext: String,
    http_headers: HttpHeaders
}


#[derive(Deserialize, Debug)]
struct PlaylistVideo
{
    title: String,
    url: String
}

#[derive(Deserialize, Debug)]
struct Playlist
{
    entries: Vec<PlaylistVideo>
}

fn get_video_from_url(url: &str) -> Result<Video,Box<dyn std::error::Error + Send + Sync>>
{
    let output = Command::new("yt-dlp")
        .arg("--dump-json")
        .arg("-f")
        .arg("best[ext=mp4]/best")
        .arg(url)
        .output()?;

    let json_content = String::from_utf8(output.stdout)?;

    let content_serialized: Video = serde_json::from_str(&json_content)?;

    Ok(content_serialized)
}

fn get_playlist_from_url(url: &str) -> Result<Playlist, Box<dyn std::error::Error + Send + Sync>>
{
    let output = Command::new("yt-dlp")
        .arg("--dump-single-json")
        .arg("--flat-playlist")
        .arg(url)
        .output()?;

    let json_content = String::from_utf8(output.stdout)?;

    let playlist_serialized: Playlist = serde_json::from_str(&json_content)?;

    Ok(playlist_serialized)
}

async fn download_hls(client: Client, url: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let response = client.get(url).send().await?
        .error_for_status()?;

    let segment_index = response.text().await?;

    let segments_urls: Vec<String> = segment_index
        .lines()
        .filter(|line| !line.starts_with("#"))
        .map(|line| line.to_string())
        .collect();

    let segments_count = segments_urls.len();

    println!("Preparing to download {} segments from YouTube.", segments_count);

    let mut file = tokio::fs::File::create(&file_path).await?;

    let chunks_count = 3; // only 3 because youtube blocks me :(
    let chunk_size = (segments_count + chunks_count - 1) / chunks_count;

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

async fn download_raw(client: Client, url: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let response = client.get(url).send().await?
        .error_for_status()?;

    let stream_length = response.content_length();
    let mut bytes_read_old: u64 = 0;
    let mut bytes_read_new: u64 = 0;
    let mut byte_stream = response.bytes_stream();

    let mut file = tokio::fs::File::create(file_path).await?;

    let mut current_time = tokio::time::Instant::now();
    while let Some(item) = byte_stream.next().await
    {
        let data = item?;
        bytes_read_new += data.len() as u64;
        file.write_all(&data).await?;
        if current_time.elapsed().as_secs() >= 1
        {
            let delta = bytes_read_new - bytes_read_old;
            bytes_read_old = bytes_read_new;
            let download_speed = delta / (1024 * 1024);

            if stream_length.is_some()
            {
                let progress_percentage = (bytes_read_new * 100) / stream_length.unwrap();
                print!("Downloaded {}% of the video. ", progress_percentage);
            }
            println!(" Currently downloading {} MiB / S.", download_speed);

            current_time = tokio::time::Instant::now();
        }
    }

    Ok(())
}

async fn download_video(video: &Video, destination: &str, semaphore: Option<Arc<Semaphore>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let mut client_headers = HeaderMap::new();
    client_headers.append("User-Agent", HeaderValue::from_str(&video.http_headers.user_agent)?);
    client_headers.append("Accept", HeaderValue::from_str(&video.http_headers.accept)?);
    client_headers.append("Accept-Language", HeaderValue::from_str(&video.http_headers.accept_language)?);
    client_headers.append("Sec-Fetch-Mode", HeaderValue::from_str(&video.http_headers.sec_fetch_mode)?);

    let client = Client::builder()
        .default_headers(client_headers)
        .build()?;

    let mut destination_copy = destination.to_string();
    if destination_copy.ends_with('/')
    {
        destination_copy.pop();
    }
    let file_name = format!("{}.{}", video.fulltitle, video.ext)
        .replace(" ", "_")
        .replace("/", "_");
    let file_path = format!("{}/{}", destination_copy, file_name);

    match tokio::fs::File::open(&file_path).await
    {
        Ok(_) =>
        {
            println!("File exists. Skipping.");
            return Ok(());
        },
        Err(_) => {},
    }

    let is_manifest = file_path.contains("manifest") || file_path.contains("m3u8");
    let _permit;
    if semaphore.is_some()
    {
        let threads_needed = if is_manifest
        {
            3
        }
        else
        {
            1
        };

        _permit = semaphore.unwrap().acquire_many_owned(threads_needed).await.unwrap();
    }
    let res = if file_path.contains("manifest") || file_path.contains("m3u8")
    {
        download_hls(client, &video.url, &file_path).await
    }
    else
    {
        download_raw(client, &video.url, &file_path).await
    };

    if let Err(err) = res
    {
        tokio::fs::remove_file(file_path).await?;
        return Err(err);
    }

    Ok(())
}

async fn video_handler(url: &String, destination: &str, semaphore: Option<Arc<Semaphore>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let _permit = match &semaphore
    {
        Some(sem) => Some(sem.acquire().await.unwrap()),
        None => None
    };

    let video: Video = get_video_from_url(url)
        .expect("Failed to download video.");

    drop(_permit);

    println!("Found video with title: {}", video.title);
    println!("Chosen format with resolution: {}", video.resolution);
    println!("Downloading video into: {}", destination);

    download_video(&video, destination, semaphore).await?;

    Ok(())
}

async fn playlist_handler(url: &String, destination: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let playlist = get_playlist_from_url(url)?;
    let playlist_len = playlist.entries.len();

    println!("Found playlist with: {} entries.", playlist.entries.len());

    let mut handles = Vec::new();
    let semaphore = Arc::new(Semaphore::new(3));
    for (playlist_video_idx, playlist_video) in playlist.entries.into_iter().enumerate()
    {
        let worker_destination = destination.to_string();
        let worker_semaphore = Arc::clone(&semaphore);
        let handle = tokio::spawn(async move
            {
                //println!("Setting up entry {} out of {} entries.", playlist_video_idx, playlist_len);
                if let Err(err) = video_handler(&playlist_video.url, &worker_destination, Some(worker_semaphore)).await
                {
                    eprintln!("Failed to download: {} with error: {}. It was entry {} out of {} entries.", playlist_video.title, err, playlist_video_idx, playlist_len);
                }
            });
        handles.push(handle);
    }

    for handle in handles
    {
        handle.await.unwrap();
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let args: Vec<String> = env::args().collect();

    if args.len() < 4
    {
        eprintln!("Invalid usage. Run: ./{} <video|playlist> <destination> <url>", &args[0]);
        return Ok(());
    }

    let command = &args[1];
    let destination = &args[2];
    let url = &args[3];

    let task: Result<(), Box<dyn std::error::Error + Send + Sync>> = match command.as_str()
    {
        "video" =>
        {
            video_handler(url, destination, None).await
        },
        "playlist" =>
        {
            playlist_handler(url, destination).await
        },
        _ =>
        {
            Err(format!("Invalid usage. Run: ./{} <video|playlist> <url>", &args[0]).into())
        }
    };

    match task
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
