# Keep direct executables while recommending archives that include all notices.
{
  arch,
  binary,
  gnutar,
  gzip,
  platform,
  runCommand,
  version,
  zip,
}:
let
  stem = "eliza-v${version}-${platform}-${arch}";
  extension = if platform == "windows" then ".exe" else "";
in
runCommand "${stem}-portable"
  {
    nativeBuildInputs = [
      gnutar
      gzip
      zip
    ];
  }
  ''
    mkdir -p "$out" "${stem}"
    cp ${binary}/bin/eliza${extension} "$out/${stem}${extension}"
    cp ${binary}/bin/eliza${extension} "${stem}/eliza${extension}"
    cp -R ${binary}/share/doc/eliza/. "${stem}/"
    ${
      if platform == "windows" then
        ''zip -qr "$out/${stem}.zip" "${stem}"''
      else
        ''tar -czf "$out/${stem}.tar.gz" "${stem}"''
    }
    # Bare executable users must download these accompanying notice files too.
    tar -czf "$out/${stem}-notices.tar.gz" -C ${binary}/share/doc/eliza .
  ''
