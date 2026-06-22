<div align="center">

<img src="screens/icon.jpg" width="180" alt="osu!mania Ranked Downloader" />

# osu!mania Ranked Downloader

A desktop app for browsing and batch-downloading ranked osu!mania beatmaps.  
No browser. No account required. Just search, filter, and download.

<p align="center">
  <img src="https://count.getloli.com/@o-mRD?theme=gelbooru" alt="Gelbooru Hit Counter" />
</p>

[![Platform](https://img.shields.io/badge/platform-Windows-blue?style=flat-square)](https://github.com/MRNORT/o-mRD/releases)
[![Latest Release](https://img.shields.io/github/v/release/MRNORT/o-mRD?style=flat-square)](https://github.com/MRNORT/o-mRD/releases)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?style=flat-square)](https://www.rust-lang.org)
[![VirusTotal Scan](https://img.shields.io/badge/VirusTotal-Clean-brightgreen?style=flat-square&logo=virustotal)](https://www.virustotal.com/gui/file/751595a57bce225d68818255669833701b9da1b17f99a2c5e498eac822ec115e)

**[Download latest release](../../releases/latest)**

</div>

---

## Screenshots

<div align="center">

<img src="screens/browse%20with%20maps%20tab.jpg" width="49%" alt="Browse tab with search results" />
<img src="screens/download%20with%20maps%20tab.jpg" width="49%" alt="Downloads tab" />

<br/><br/>

<img src="screens/settings%20tab.jpg" width="49%" alt="Settings tab" />

</div>

---

## Features

- **Search** ranked mania maps by key count (4K-8K), star rating, BPM, and text query
- **Batch download** - filter for exactly what you want, then hit "Download All Listed"
- **Mirror fallback** - tries Nerinyan, BeatConnect, and Chimu automatically if one fails
- **No video option** - skip the video track to save space and avoid failed downloads on video maps
- **Auto-import** - drops `.osz` files directly into your osu!/Songs folder if it's detected
- **Already-have tracking** - scans your Songs folder so it won't re-download what you already have
- **Audio preview** - click a map thumbnail to hear the preview track
- **No API key required** - works out of the box using the Nerinyan mirror

---

## Download

Go to the **[Releases](../../releases/latest)** page and grab `o!mRD.exe`.

No installer. Just run the `.exe` - it's portable and self-contained.

---

## Usage

### Browse tab

Set your filters on the left panel:

| Filter | Description |
|--------|-------------|
| Key Count | Toggle 4K, 5K, 6K, 7K, 8K (or leave blank for all) |
| Star Rating | Min/max difficulty range |
| BPM | Min/max BPM range |
| Sort By | Newest ranked, most played, hardest, etc. |
| Search | Title, artist, or mapper name |

Hit **Search**, then:
- Click **Download** on individual maps
- Or click **Download All Listed** to queue everything shown at once

### Downloads tab

Shows all queued and active downloads with their status. Use **Open Folder** to go to your download directory.

### Settings tab

- **osu! API (optional)** - By default the app uses the Nerinyan API with no account needed. If you want the official osu! API, create an OAuth client at [osu.ppy.sh/home/account/edit#oauth](https://osu.ppy.sh/home/account/edit#oauth) and paste your credentials here.
- **Download Directory** - Where `.osz` files are saved (defaults to a `downloads` folder next to the exe)
- **osu! Integration** - Set your osu! folder for auto-import directly into Songs
- **Prefer no video** - Downloads the no-video version of beatmaps to save space

---

## Building from source

Requires [Rust](https://rustup.rs) (stable toolchain).

```sh
git clone https://github.com/MRNORT/o-mRD
cd osu-mania-dl
cargo build --release
```

Binary will be at `target/release/o!mRD.exe`.

---

## Known limitations

- "Download All Listed" downloads what's currently shown - narrow your search first if you want a specific set
- Pagination is not yet supported in batch mode
- Auto-import requires osu! to be at the default install path, or configured manually in Settings

---

## License

MIT
