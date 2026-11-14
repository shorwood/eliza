# Package the tested Nix server as a Docker-compatible image archive.
# Building the archive needs no Docker daemon; release CI loads and publishes it.
{
  cacert,
  dockerTools,
  lib,
  package,
}:
dockerTools.buildLayeredImage {
  name = "eliza";
  tag = package.version;

  # --- Assemble the minimal filesystem from Nix store paths.
  # No base image is declared. buildLayeredImage brings in the program, CA
  # certificates, and their runtime dependencies; there is no shell or apk/apt.
  contents = [
    package
    cacert
  ];

  # --- Define how the image starts and how it is identified in registries.
  config = {
    # The server needs CA roots for outbound HTTPS requests.
    Env = [ "SSL_CERT_FILE=${cacert}/etc/ssl/certs/ca-bundle.crt" ];
    User = "65532:65532";
    Entrypoint = [ (lib.getExe package) ];
    Cmd = [
      "serve"
      "--bind"
      "0.0.0.0:8787"
    ];
    ExposedPorts = {
      "8787/tcp" = { };
    };
    StopSignal = "SIGTERM";

    # OCI labels are metadata for registries and image inspection tools.
    Labels = {
      "org.opencontainers.image.title" = "ELIZA Compatibility Server";
      "org.opencontainers.image.description" = "1966 ELIZA through AI provider-compatible HTTP APIs";
      "org.opencontainers.image.licenses" = "MIT AND Apache-2.0 AND LicenseRef-CMU";
      "org.opencontainers.image.source" = "https://github.com/shorwood/eliza";
      "org.opencontainers.image.version" = package.version;
    };
  };
}
