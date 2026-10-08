{
  apk-tools,
  binary,
  runCommand,
  version,
  arch,
}:
runCommand "eliza-v${version}-linux-${arch}-apk"
  {
    nativeBuildInputs = [ apk-tools ];
  }
  ''
    mkdir -p "$out" package/usr/bin package/usr/share/licenses/eliza
    cp ${binary}/bin/eliza package/usr/bin/eliza
    cp -R ${binary}/share/licenses/eliza/. package/usr/share/licenses/eliza/
    apk mkpkg --compat 2.14.0 \
      --files package \
      --info name:eliza \
      --info version:${version}-r0 \
      --info arch:${arch} \
      --info description:'1966 ELIZA through AI provider-compatible HTTP APIs' \
      --info 'license:MIT AND Apache-2.0' \
      --info url:https://github.com/shorwood/eliza \
      --output "$out/eliza-v${version}-linux-${arch}.apk"
  ''
