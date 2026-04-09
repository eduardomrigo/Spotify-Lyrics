# Spotify Lyrics

A lightweight, always-on-top desktop overlay that displays synced lyrics for your currently playing Spotify track.

Built with [Tauri](https://tauri.app/) (Rust backend) and vanilla HTML/JS frontend.

## Features

- Real-time synced lyrics from [LRCLIB](https://lrclib.net/)
- Transparent, always-on-top overlay window
- Customizable opacity, accent color, font size, and visible lines
- Smart lyric matching (handles remasters, features, remixes)
- Spotify OAuth authentication

## Prerequisites

- [Rust](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (for Tauri CLI)
- A [Spotify Developer](https://developer.spotify.com/dashboard) application

## Spotify Setup

1. Go to the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard)
2. Create a new application
3. Add `http://127.0.0.1:8888/callback` as a Redirect URI
4. Copy the **Client ID** and **Client Secret**

## Installation

```bash
# Clone the repo
git clone https://github.com/your-username/Lyrics-Tauri.git
cd Lyrics-Tauri

# Install JS dependencies
npm install

# Run in dev mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Usage

1. Launch the app
2. Enter your Spotify **Client ID** and **Client Secret**
3. Authorize with Spotify via browser
4. Play a song on Spotify — lyrics will appear automatically

## License

[MIT](LICENSE)
