# Smoke-test Linux/macOS downloads on their native build hosts.
# The Windows executable is tested by the separate Windows CI job.
{
  apk-tools,
  arch,
  assets,
  binutils,
  darwin,
  dpkg,
  gnutar,
  isLinux,
  lib,
  runCommand,
  version,
}:
runCommand "eliza-release-check"
  {
    # --- Add the inspection and package tools needed on this host.
    # optionals contributes only the tools for the active operating system.
    nativeBuildInputs =
      lib.optionals isLinux [
        apk-tools
        binutils
        dpkg
      ]
      ++ lib.optionals (!isLinux) [
        darwin.cctools
        gnutar
      ];
  }
  ''
    # --- Locate the executable inside the downloaded artifact.
    ${
      if isLinux then
        ''
          tar -xzf ${assets}/eliza-v${version}-linux-${arch}.tar.gz
          binary=./eliza-v${version}-linux-${arch}/eliza
        ''
      else
        ''
          tar -xzf ${assets}/eliza-v${version}-macos-${arch}.tar.gz
          binary=./eliza-v${version}-macos-${arch}/eliza
        ''
    }

    # --- Start the executable before inspecting portability or package payloads.
    # This catches missing runtime libraries that a successful build may miss.
    "$binary" --help >/dev/null
    stem=eliza-v${version}-${if isLinux then "linux" else "macos"}-${arch}
    test -s "$stem/LICENSE"
    test -s "$stem/licences/flite-rs/LICENSE"
    grep -q 'Carnegie Mellon University' "$stem/licences/flite-rs/NOTICE"
    grep -q 'CMU license' "$stem/licences/flite-rs/THIRD-PARTY-LICENSES.md"
    test -s "$stem/licences/scalar/LICENSE"
    ${
      if isLinux then
        ''
          # --- Reject dynamic loading in the standalone Linux download.
          ! readelf -l "$binary" | grep -q INTERP

          # --- Install both package formats and run their payloads.
          mkdir deb-root apk-root
          dpkg-deb --extract ${assets}/eliza-v${version}-linux-${arch}.deb deb-root
          apk --root "$PWD/apk-root" --allow-untrusted add --initdb --usermode \
            ${assets}/eliza-v${version}-linux-${arch}.apk
          deb-root/usr/bin/eliza --help >/dev/null
          apk-root/usr/bin/eliza --help >/dev/null
          for root in deb-root apk-root; do
            test -s "$root/usr/share/doc/eliza/LICENSE"
            test -s "$root/usr/share/doc/eliza/licences/flite-rs/NOTICE"
            test -s "$root/usr/share/doc/eliza/licences/flite-rs/THIRD-PARTY-LICENSES.md"
            test -s "$root/usr/share/doc/eliza/licences/scalar/LICENSE"
          done
          cmp "$binary" ${assets}/eliza-v${version}-linux-${arch}
        ''
      else
        ''
          # --- Reject links into the Nix store in the macOS download.
          ! otool -L "$binary" | grep -q /nix/store/
        ''
    }
    touch "$out"
  ''
