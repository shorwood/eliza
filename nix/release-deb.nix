{
  binary,
  dpkg,
  runCommand,
  version,
  arch,
}:
runCommand "eliza-v${version}-linux-${arch}-deb"
  {
    nativeBuildInputs = [ dpkg ];
  }
  ''
      mkdir -p "$out" package/DEBIAN package/usr/bin package/usr/share/doc/eliza
      cp ${binary}/bin/eliza package/usr/bin/eliza
      cp -R ${binary}/share/licenses/eliza/. package/usr/share/doc/eliza/
      cat > package/DEBIAN/control <<EOF
    Package: eliza
    Version: ${version}
    Section: utils
    Priority: optional
    Architecture: ${arch}
    Maintainer: Stanley Horwood <stanley@hsjm.io>
    Description: 1966 ELIZA through AI provider-compatible HTTP APIs
    EOF
      dpkg-deb --build --root-owner-group package "$out/eliza-v${version}-linux-${arch}.deb"
  ''
