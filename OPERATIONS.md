# Private distribution

The implementation repository is `shorwood/eliza`. Public documentation and a
hosted service are follow-up work; there is no hosted API URL to use yet.
Repository privacy and software licensing are separate: the existing MIT license
remains in place. Earlier distributed copies retain their license.

Repository visibility was changed to private on 2026-10-07. Anonymous GitHub API
access returns 404; authorized SSH access works and Actions remains enabled.
Merge the private release guards before creating another release tag.

## Source access

Repository collaborators clone with GitHub SSH credentials:

```sh
git clone git@github.com:shorwood/eliza.git
cd eliza
nix run . -- serve
```

CI checks out the private repository with its scoped `GITHUB_TOKEN`; it does not
need a personal access token for source checkout. Remote Nix `github:` URLs no
longer work anonymously. Use an authorized checkout rather than putting a token
in a command-line URL or configuration committed to the repository.

## Container access

Releases keep the name `ghcr.io/shorwood/eliza`. The public package was deleted on
2026-10-07 after the operator enabled the required credential scopes. The deletion
request succeeded, and an authenticated package lookup returned 404. GitHub cannot
turn a public package private; the next release recreates it with private visibility.
Existing registry versions, including v1.0.0 and `latest`, were removed; downloaded
copies remain usable.

For an operator pull, grant package read access and authenticate using a personal
access token (classic) with `read:packages`. Obtain credentials through your secret
manager; never place a token in source, a command-line URL, or logs. Docker can
store login credentials, so use a configured credential helper on serving hosts.

```sh
printf '%s' "$ELIZA_GHCR_TOKEN" | docker login ghcr.io \
  --username "$ELIZA_GHCR_USER" --password-stdin
docker pull ghcr.io/shorwood/eliza:<release>
docker run --rm -p 127.0.0.1:8787:8787 ghcr.io/shorwood/eliza:<release>
```

Replace `<release>` with a published version. After deletion, the package does not
exist until its first successful private release. Earlier tags are not restored.
Production deployment should pin the resulting image digest.

## Releases and verification

The release workflow requires a private source repository and either a missing
destination package or an existing private one. An authorization/API failure
other than package-not-found blocks publication. A newly created personal-account
package defaults to private; the workflow verifies visibility after each upload
before creating the multi-architecture manifest and release.

Preserve the image's source label so new packages link to the implementation
repository. Confirm inherited repository access and Actions permissions in the
package settings; visibility is independent of repository access inheritance.

After the first release, verify from a clean operator environment:

1. Authenticated SSH clone and checkout succeed.
2. Private CI and release checks pass.
3. An authenticated pull succeeds and the container's `/healthz` responds.
4. An anonymous package/manifest request cannot retrieve the new image.
5. Both amd64 and arm64 manifest entries exist; record the deployment digest.

The repository visibility change does not upload local workflow edits. Merge/push
the reviewed workflow before creating another release tag. Public-docs publication,
Vercel access, origin deployment, and billing belong to their separate proposals.

## Recovery

Do not make the repository or package public as an incident workaround. Restore
collaborator/Actions access or operator credentials, then retry the private build
or pull. A missing new release can use the last checked private digest. Before the
first private release, build from an authorized checkout with `nix build .#dockerImage`.

References: [GitHub package access and visibility](https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility)
and [GHCR authentication](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry).
