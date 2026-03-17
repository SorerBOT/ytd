# ytd
ytd is a CLI tool made for downloading YouTube videos and playlists, written in Rust.  
Under the hood it uses yt-dlp to find the video / playlist metadata, then handles the video download itself.

## Features:
* Playlist state persistence: if you get rate-limited while downloading a playlist, a fresh download (into the same folder) will only install the missing files, skipping the ones already downloaded.
* Fault tolerance: playlist downloads don't pause after a single video crashed. They skip it.
* It handles both HLS manifests and raw MP4 files.
* Maximises parallelism (within YouTube rate-limit constraints).

## Dependencies:
* yt-dlp - can be downloaded with most package managers.

## How to use?
Build using:
```bash
cargo build --release
```
To download a video:
```bash
./target/release/ytd video <destination_folder> <url>
```
To download a playlist:
```bash
./target/release/ytd playlist <destination_folder> <url>
```
