# Wrap the portable Linux binary in an Alpine package.
# `arch` names the download; `packageArch` is Alpine's package metadata value.
{
  apk-tools,
  binary,
  runCommand,
  version,
  arch,
  packageArch,
}:
runCommand "eliza-v${version}-linux-${arch}-apk"
  {
    nativeBuildInputs = [ apk-tools ];
  }
  ''
    # --- Stage the executable in Alpine's directory layout.
    mkdir -p "$out" package/usr/bin
    cp ${binary}/bin/eliza package/usr/bin/eliza
    mkdir -p package/usr/share/doc
    cp -R ${binary}/share/doc/eliza package/usr/share/doc/

    # --- Write package metadata and turn the staged tree into one .apk file.
    apk mkpkg --compat 2.14.0 \
      --files package \
      --info name:eliza \
      --info version:${version}-r0 \
      --info arch:${packageArch} \
      --info description:'1966 ELIZA through AI provider-compatible HTTP APIs' \
      --info 'license:MIT AND Apache-2.0' \
      --info url:https://github.com/shorwood/eliza \
      --output "$out/eliza-v${version}-linux-${arch}.apk"
  ''
