{
  apk,
  apk-tools,
  archive,
  binutils,
  darwin,
  deb,
  dpkg,
  gnutar,
  isLinux,
  lib,
  runCommand,
}:
runCommand "eliza-release-check"
  {
    nativeBuildInputs = [
      gnutar
    ]
    ++ lib.optionals isLinux [
      apk-tools
      binutils
      dpkg
    ]
    ++ lib.optionals (!isLinux) [ darwin.cctools ];
  }
  ''
    archive_file=$(find ${archive} -name '*.tar.gz' -print -quit)
    tar -xzf "$archive_file"
    binary=$(find . -name eliza -type f -print -quit)
    "$binary" --help >/dev/null
    ${
      if isLinux then
        ''
          ! readelf -l "$binary" | grep -q INTERP
          mkdir deb-root apk-root
          dpkg-deb --extract "$(find ${deb} -name '*.deb' -print -quit)" deb-root
          apk --root "$PWD/apk-root" --allow-untrusted add --initdb --usermode \
            "$(find ${apk} -name '*.apk' -print -quit)"
          deb-root/usr/bin/eliza --help >/dev/null
          apk-root/usr/bin/eliza --help >/dev/null
        ''
      else
        ''
          ! otool -L "$binary" | grep -q /nix/store/
        ''
    }
    touch "$out"
  ''
