#!/bin/sh
# Build Rex Inbox.app into ~/Applications, sign it for this Mac, and
# (re)start it. Run it again after pulling changes.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
app="$HOME/Applications/Rex Inbox.app"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/../Cargo.toml" | head -1)

swift build -c release --package-path "$here"
bin="$(swift build -c release --package-path "$here" --show-bin-path)/RexInbox"

pkill -x RexInbox 2>/dev/null || true
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/RexInbox"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>io.github.captjt.rex-inbox.app</string>
  <key>CFBundleName</key>
  <string>Rex Inbox</string>
  <key>CFBundleDisplayName</key>
  <string>Rex Inbox</string>
  <key>CFBundleExecutable</key>
  <string>RexInbox</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

# Ad hoc: signed for this Mac only. Turn on Open at Login from the app.
codesign --force --sign - "$app"
open "$app"
echo "installed $app"
