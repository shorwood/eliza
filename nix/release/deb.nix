# Wrap the portable Linux binary in a Debian package.
# `arch` names the download; `packageArch` is Debian's control-file spelling.
{
  binary,
  dpkg,
  runCommand,
  version,
  arch,
  packageArch,
}:
runCommand "eliza-v${version}-linux-${arch}-deb"
  {
    nativeBuildInputs = [ dpkg ];
  }
  ''
      # --- Stage the executable in Debian's directory layout.
      mkdir -p "$out" package/DEBIAN package/usr/bin
      cp ${binary}/bin/eliza package/usr/bin/eliza
      mkdir -p package/usr/share/doc
      cp -R ${binary}/share/doc/eliza package/usr/share/doc/

      # --- Tell dpkg which package and CPU architecture these files belong to.
      cat > package/DEBIAN/control <<EOF
    Package: eliza
    Version: ${version}
    Section: utils
    Priority: optional
    Architecture: ${packageArch}
    Maintainer: Stanley Horwood <stanley@hsjm.io>
    Description: 1966 ELIZA through AI provider-compatible HTTP APIs
    EOF

      # --- Emit one .deb download; root ownership is normalized for packaging.
      dpkg-deb --build --root-owner-group package "$out/eliza-v${version}-linux-${arch}.deb"
  ''
