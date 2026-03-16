use std::process::Command;

fn download_video(url: &str) -> Result<(),Box<dyn std::error::Error>>
{
    let output = Command::new("yt-dlp")
        .arg("--dump-json")
        .arg("-f")
        .arg("best[ext=mp4]")
        .arg(url)
        .output()?;

    let json_content = String::from_utf8(output.stdout)?;

    print!("{}", json_content);



    Ok(())
}

fn main()
{
    let url = "https://www.youtube.com/watch?v=NV401gLmiAk";

    download_video(url)
        .expect("Failed to download video.");
}
