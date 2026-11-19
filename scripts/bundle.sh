#!/bin/sh
# Builds Pointer.app for Apple silicon and Intel, signs it with Developer ID and the hardened
# runtime, notarizes and staples it, then packs what a release carries:
#   dist/Pointer.zip  the app, zipped: what the in-app updater downloads (keep this name)
#   dist/Pointer.dmg  the app beside an Applications link: the website's download
#
#   scripts/bundle.sh                 sign and notarize
#   POINTER_SIGN=- scripts/bundle.sh  ad-hoc signed, not notarized: for trying the bundle locally
#
# Signing uses $POINTER_SIGN, else the keychain's "Developer ID Application" identity.
# Notarizing uses the notarytool keychain profile $NOTARY_PROFILE (default "pointer-notary";
# make one with `xcrun notarytool store-credentials pointer-notary --key … --key-id … --issuer …`).
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || . "$HOME/.cargo/env"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
dist=dist
app="$dist/Pointer.app"
profile="${NOTARY_PROFILE:-pointer-notary}"
identity="${POINTER_SIGN:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -n 1)}"
if [ -z "$identity" ]; then
    echo "bundle: no Developer ID Application identity in the keychain (POINTER_SIGN=- for an ad-hoc build)" >&2
    exit 1
fi

echo "bundle: Pointer $version"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$target" >/dev/null 2>&1 || true
    MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --quiet --target "$target"
done

rm -rf "$dist" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/Pointer" \
    target/aarch64-apple-darwin/release/pointer target/x86_64-apple-darwin/release/pointer
# Local symbols only bloat the download (about a fifth of it); crash reports still symbolicate.
strip -x "$app/Contents/MacOS/Pointer"
sed "s/@VERSION@/$version/g" scripts/Info.plist.in > "$app/Contents/Info.plist"
plutil -lint -s "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

if [ "$identity" = "-" ]; then
    codesign --force --sign - "$app"
    ditto -c -k --keepParent "$app" "$dist/Pointer.zip"
    echo "bundle: ad-hoc build in $dist (not notarized)"
    exit 0
fi

echo "bundle: signing as $identity"
codesign --force --options runtime --timestamp --sign "$identity" "$app"
codesign --verify --deep --strict "$app"

echo "bundle: notarizing the app"
ditto -c -k --keepParent "$app" "$dist/notarize.zip"
xcrun notarytool submit "$dist/notarize.zip" --keychain-profile "$profile" --wait
rm "$dist/notarize.zip"
xcrun stapler staple -q "$app"
ditto -c -k --keepParent "$app" "$dist/Pointer.zip"

echo "bundle: building the disk image"
stage="$dist/dmg"
mkdir -p "$stage"
ditto "$app" "$stage/Pointer.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Pointer" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dist/Pointer.dmg"
rm -rf "$stage"
codesign --force --timestamp --sign "$identity" "$dist/Pointer.dmg"
echo "bundle: notarizing the disk image"
xcrun notarytool submit "$dist/Pointer.dmg" --keychain-profile "$profile" --wait
xcrun stapler staple -q "$dist/Pointer.dmg"

spctl --assess --type execute --verbose=2 "$app"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dist/Pointer.dmg"
(cd "$dist" && shasum -a 256 Pointer.zip Pointer.dmg > SHA256SUMS.txt && cat SHA256SUMS.txt)
echo "bundle: done: $dist/Pointer.zip and $dist/Pointer.dmg"
