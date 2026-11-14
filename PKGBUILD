# Maintainer: Ryne
pkgname=melissm
pkgver=0.1.4
pkgrel=1
pkgdesc="It plays music, literally. what else would it do"
arch=('x86_64')
url="https://github.com/rynekryz/melissm"
license=('GPL-3.0-or-later')
depends=('webkit2gtk-4.1' 'gtk3' 'libappindicator-gtk3' 'librsvg' 'gst-plugins-good' 'gst-plugins-bad' 'gst-libav' 'matugen')
makedepends=('nodejs' 'npm' 'clang' 'lld' 'base-devel')
source=()
sha256sums=()

prepare() {
  command -v cargo >/dev/null || . "$HOME/.cargo/env"
  export PATH="$HOME/.cargo/bin:$PATH"
  cd "$startdir"
  npm install --no-audit --no-fund
}

build() {
  export PATH="$HOME/.cargo/bin:$PATH"
  unset CFLAGS CXXFLAGS LDFLAGS CPPFLAGS
  cd "$startdir"
  npm run tauri build -- --no-bundle
}

package() {
  cd "$startdir"
  install -Dm755 src-tauri/target/release/melissm "$pkgdir/usr/bin/melissm"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 icons/icon.svg "$pkgdir/usr/share/icons/hicolor/scalable/apps/melissm.svg"
  install -Dm644 icons/icon-512.png "$pkgdir/usr/share/icons/hicolor/512x512/apps/melissm.png"
  install -Dm644 melissm.desktop "$pkgdir/usr/share/applications/melissm.desktop"
}
