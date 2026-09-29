# This produces a reproducible Docker-compatible archive without contacting a
# container daemon. Docker or Podman is only needed later to load and run it.
{
  dockerTools,
  lib,
  package,
}:
dockerTools.buildLayeredImage {
  name = "eliza";
  tag = package.version;

  # With no base image, the result contains only ELIZA and its Nix runtime
  # closure. There is intentionally no shell or package manager in the image.
  contents = [ package ];
  config = {
    User = "65532:65532";
    Entrypoint = [ (lib.getExe package) ];
    Cmd = [
      "serve"
      "--host"
      "0.0.0.0"
    ];
    ExposedPorts = {
      "8787/tcp" = { };
    };
    StopSignal = "SIGTERM";
    Labels = {
      "org.opencontainers.image.title" = "ELIZA Compatibility Server";
      "org.opencontainers.image.description" = "Classic ELIZA through provider-compatible HTTP APIs";
      "org.opencontainers.image.licenses" = "MIT";
      "org.opencontainers.image.version" = package.version;
    };
  };
}
