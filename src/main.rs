use std::process::Command;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct Format
{
    ext: String,
    video_ext: String,
    url: String,
    resolution: String,
    width: Option<u32>,
    height: Option<u32>
}

#[derive(Deserialize, Debug)]
struct Video
{
    title: String,
    formats: Vec<Format>
}

fn is_valid_format(format: &Format) -> bool
{
    format.ext == "mp4"
        && format.video_ext == "mp4"
        && format.width.is_some()
        && format.height.is_some()
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

    let mut content_serialized: Video = serde_json::from_str(&json_content)?;

    content_serialized.formats.retain(is_valid_format);

    assert_ne!(content_serialized.formats.len(), 0);

    content_serialized.formats.sort_by(|f1, f2|
        {
            let f1_area = f1.width.unwrap() * f1.height.unwrap();
            let f2_area = f2.width.unwrap() * f2.height.unwrap();
            f2_area.cmp(&f1_area)
        });

    content_serialized.formats.truncate(1);

    Ok(content_serialized)
}

fn main()
{
    let url = "https://www.youtube.com/watch?v=NV401gLmiAk";

    let video: Video = get_video_from_url(url)
        .expect("Failed to download video.");

    println!("Found video with title: {}", video.title);
    println!("Chosen format with resolution: {}", video.formats[0].resolution);


}
