# Fetch the browser bundle used by the embedded /docs API reference.
# The version and content hash pin the asset without committing scalar.js.
{ fetchurl }:
fetchurl {
  url = "https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.70.0/dist/browser/standalone.js";
  hash = "sha256-q2VPlzpeWqW6DLD1NjXbjJsqDztFSmEyOMT6IjsgRuI=";
}
