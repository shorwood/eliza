# Bundle the native macOS executable in one .tar.gz download.
{
  binary,
  arch,
  gnutar,
  gzip,
  runCommand,
  version,
}:
let
  # Reuse the stem for both the archive name and its top-level directory.
  stem = "eliza-v${version}-macos-${arch}";
in
runCommand "${stem}-archive"
  {
    nativeBuildInputs = [
      gnutar
      gzip
    ];
  }
  ''
    # --- Stage the executable under a predictable directory name.
    mkdir -p "$out" "${stem}"
    cp ${binary}/bin/eliza "${stem}/"
    cp -R ${binary}/share/doc/eliza/. "${stem}/"

    # --- Create the single file that CI publishes as a download.
    tar -czf "$out/${stem}.tar.gz" "${stem}"
  ''
