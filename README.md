<div align="center">

<!-- decor:top -->
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sky-banner-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sky-banner-light.svg" width="100%" alt="night sky"></picture>

<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-light.svg" width="100%" alt=""></picture>
<!-- /decor:top -->

<img src="icons/icon.png" width="160" height="160" style="display: block; margin: 0 auto"/>
<h1>Melissm</h1>
<p>It plays music, literally. What else would it do?</p>
<p>A music app for Arch Linux with streaming, synced lyrics, and a Material 3 Expressive UI that themes itself from your wallpaper.</p>
</div>
<p align="center">
  <a href="https://github.com/rynekryz/melissm/stargazers"><img src="assets/badges/stars.svg" alt="GitHub stars" height="30"/></a>
  <a href="https://github.com/rynekryz/melissm/tags"><img src="assets/badges/release.svg" alt="Latest release" height="30"/></a>
  <a href="LICENSE"><img src="assets/badges/licencegplv3.svg" alt="Licence GPLv3"/></a>
  <img src="assets/badges/arch.svg" alt="Arch"/>
  <img src="assets/badges/rust.svg" alt="Rust"/>
  <img src="assets/badges/astro.svg" alt="Astro"/>
</p>

<p align="center">
  <a href="https://github.com/rynekryz/melissm/releases"><img src="assets/badges/getgithub.svg" alt="Get it on GitHub" height="56"/></a>
</p>

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-sparkles-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-sparkles-light.svg" width="100%" alt=""></picture>
</div>

## Screenshots

<div align="center">
<img src="/Screenshots/151240.png" width="49%" />
<img src="/Screenshots/151036.png" width="49%" />
</div>

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-light.svg" width="100%" alt=""></picture>
</div>

## Features

- Tauri v2 + Rust backend, no Electron and no bundled Chromium, your RAM is safe here
- Streams audio straight from YouTube Music, no video overhead, the GPU can chill
- Live Material 3 Expressive theming from your wallpaper via Matugen, or any color you like
- Caelestia scheme fallback when wallpaper extraction gives up
- MPRIS controls, so it shows up in your media widget with lyrics
- Search, Home, Album, Artist, Playlist, Library and History
- Native queue: play next, add to queue, reorder, radio, clear upcoming
- Gapless playback and optional crossfade
- Playlists, liked songs and history stored locally
- Synced lyrics from LRCLIB with word-by-word timing, NetEase as backup
- Write your own lyrics or fix the timing with an offset, fullscreen too
- Tray controls, so you can pause when the boss walks by
- Speed, pitch, equalizer and audio quality
- Played songs get cached for instant replay
- Single instance, clicking again just focuses the window
- Material Web Components + Tailwind CSS 4
- Desktop entry and GTK app id so your launcher knows it exists
- It plays music, literally
- etc.

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-light.svg" width="100%" alt=""></picture>
</div>

## Technical Stack

- **Shell:** Astro with Tauri v2 (Rust)
- **UI:** Material Web Components and Tailwind CSS 4
- **Playback:** GStreamer
- **Database:** SQLite through rusqlite
- **Network:** reqwest
- **Theming:** Matugen
- **Language:** TypeScript, Astro and Rust
- **Build:** npm, Cargo and a PKGBUILD

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-sparkles-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-sparkles-light.svg" width="100%" alt=""></picture>
</div>

## Building

You'll need Arch Linux (or an Arch-based distro, you know who you are) and Git.

### From the PKGBUILD

```bash
git clone https://github.com/rynekryz/melissm.git
cd melissm
makepkg -si
```

### From source

```bash
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg alsa-lib nodejs npm rust matugen gstreamer gst-plugins-good gst-plugins-bad gst-libav
git clone https://github.com/rynekryz/melissm.git
cd melissm
npm install
npm run tauri dev
npm run tauri build
```

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-light.svg" width="100%" alt=""></picture>
</div>

## Project Structure

```
Melissm/
├── src/
│   ├── components/
│   ├── layouts/
│   ├── lib/
│   ├── pages/
│   └── styles/
├── src-tauri/
│   └── src/
├── icons/
├── public/
└── PKGBUILD
```

<div align="center">
<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-dots-light.svg" width="100%" alt=""></picture>
</div>

## Legal

Melissm is an independent open-source music player, not affiliated with or endorsed by YouTube or Google LLC. All trademarks belong to their owners. It doesn't host or distribute any media; playback streams straight from YouTube Music.

## Credits

Extractor approach and lyrics provider design from [Metrolist](https://github.com/MetrolistGroup/Metrolist). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the full list.

## License

Licensed under GPL-3.0-or-later. See [LICENSE](LICENSE).

<!-- decor:bottom -->
<div align="center">

<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/divider-wave-light.svg" width="100%" alt=""></picture>

<picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-heart-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-heart-light.svg" width="60" alt="heart"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-sparkle-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-sparkle-light.svg" width="60" alt="sparkle"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-moon-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-moon-light.svg" width="60" alt="moon"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-flower-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-flower-light.svg" width="60" alt="flower"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-cat-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-cat-light.svg" width="60" alt="cat"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-ghost-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-ghost-light.svg" width="60" alt="ghost"></picture> <picture><source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-lock-dark.svg"><img src="https://raw.githubusercontent.com/rynekryz/rynekryz/main/pixel-art/sticker-lock-light.svg" width="60" alt="lock"></picture>

</div>
<!-- /decor:bottom -->

<div align="center">
<strong>Made with ♥︎ by Ryne</strong>
</div>
<div align="center">
  <a href="https://discord.gg/YKhWXXhKmC"><img src="assets/badges/discord.svg" alt="Discord"/></a>
  <a href="https://tiktok.com/@rynekryz"><img src="assets/badges/tiktok.svg" alt="TikTok"/></a>
</div>
