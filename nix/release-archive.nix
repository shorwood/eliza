{
  binary,
  gnutar,
  gzip,
  runCommand,
  zip,
  version,
  platform,
  isWindows ? false,
}:
let
  stem = "eliza-v${version}-${platform}";
  extension = if isWindows then "zip" else "tar.gz";
in
runCommand "${stem}-archive"
  {
    nativeBuildInputs =
      if isWindows then
        [ zip ]
      else
        [
          gnutar
          gzip
        ];
  }
  ''
    mkdir -p "$out" "${stem}"
    cp ${binary}/bin/eliza${if isWindows then ".exe" else ""} "${stem}/"
    cp -R ${binary}/share/licenses/eliza/. "${stem}/"
    ${
      if isWindows then
        ''zip -qr "$out/${stem}.${extension}" "${stem}"''
      else
        ''tar -czf "$out/${stem}.${extension}" "${stem}"''
    }
  ''
