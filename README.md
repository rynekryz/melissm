<div align="center">
<img src="icons/icon.png" width="160" height="160" style="display: block; margin: 0 auto"/>
<h1>Melissm</h1>
<p>It plays music, literally. what else would it do</p>
<p>A music streaming app for desktop. Astro + Tauri v2 (Rust), GStreamer playback, Material 3 Expressive UI. Themed live from your wallpaper via Matugen on Linux, or your Windows accent color on Windows (falls back to Caelestia's scheme).</p>
</div>

## Installing

### Linux (Arch)
Requirements:
- Arch Linux or an Arch-based distro (you know who you are)
- Git

Install from PKGBUILD:
```bash
git clone https://github.com/rynekryz/melissm.git
cd melissm
makepkg -si
```

Or build from source, install deps first:
```bash
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg alsa-lib nodejs npm rust matugen gstreamer gst-plugins-good gst-plugins-bad gst-libav
```

Then:
```bash
git clone https://github.com/rynekryz/melissm.git
cd melissm
npm install
npm run tauri dev
npm run tauri build
```

### Windows
Requirements:
- Node.js 20+
- Rust with the MSVC toolchain
- Visual Studio Build Tools (C++ workload)
- GStreamer MSVC runtime (add its `bin` folder to PATH, needed for audio)

Then:
```bash
git clone https://github.com/rynekryz/melissm.git
cd melissm
npm install
npm run tauri build
```

The NSIS installer lands in `src-tauri/target/release/bundle/nsis/`, and the portable exe is `src-tauri/target/release/melissm.exe`. Or just grab both from the releases page, no building required.

## Features

- Tauri v2 + Rust backend, no Electron, no bundled Chromium, your RAM is safe here
- Streams audio straight from YouTube Music, no video overhead, the GPU can chill
- Live Material 3 Expressive theming: wallpaper via Matugen on Linux, Windows accent color on Windows
- Caelestia scheme fallback when wallpaper extraction doesn't work out
- Search, Home, Album, Library and History
- Synced lyrics panel via LrcLib, slides in like it's cool
- System tray with playback controls, so you can pause when the boss walks by
- Play / pause / resume / seek / volume
- Material Web Components + Tailwind CSS 4
- Desktop entry and GTK app id so your app launcher actually knows it exists
- It plays music, literally
- etc.

## Credits

Extractor approach and lyrics provider design from [Metrolist](https://github.com/MetrolistGroup/Metrolist). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the full list.

## License

Licensed under GPL-3.0-or-later. See [LICENSE](LICENSE) for details.

<div align="center">
<strong>Made with ♥︎ by Ryne</strong>
</div>
<div align="center">
  <a href="DISCORD_INVITE"><img src="https://img.shields.io/badge/Discord-5A5A5A?style=for-the-badge&logo=discord&logoColor=white"/></a>
  <a href="https://tiktok.com/@rynekryz"><img src="https://img.shields.io/badge/TikTok-5A5A5A?style=for-the-badge&logo=tiktok&logoColor=white"/></a>
</div>
