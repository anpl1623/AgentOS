# Releasing

A release is cut by pushing a tag. Everything published is built from that tag by
[`.github/workflows/release.yml`](../.github/workflows/release.yml), never from a maintainer's
machine, so what ships is reproducible from the repository.

## Cutting one

1. Move the entries under `## [Unreleased]` in [`CHANGELOG.md`](../CHANGELOG.md) into a section for
   the new version, and add the link at the bottom of the file.

2. Set the version in all three places. They must agree — `cargo test` fails if they do not, and the
   release workflow refuses to publish if they do not:

   - `Cargo.toml`, `[workspace.package] version`
   - `apps/desktop/src-tauri/tauri.conf.json`, `version`
   - `apps/desktop/package.json`, `version`

3. Run `cargo build` so `Cargo.lock` picks up the new version, then commit and merge to `main`.

4. Tag and push:

   ```bash
   git tag -a v0.1.0 -m "AgentOS v0.1.0"
   git push origin v0.1.0
   ```

The workflow then checks the tag against the tree, creates a draft release, builds the desktop
bundles for macOS, Windows and Linux and the `agentos` CLI for four targets, uploads everything with
checksums, and publishes the draft only once every platform has produced an artifact. A release
missing the Windows installer for twenty minutes is worse than one that appears late.

Running the workflow manually from the Actions tab builds every artifact and attaches them to the
workflow run instead of publishing anything. Use it to exercise packaging changes.

## Signing

The workflow signs and notarises the macOS bundles when these repository secrets are present, and
builds them unsigned when they are not. It never fails for want of a certificate: an unsigned build
somebody can verify by hand is more useful than no build at all.

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64 of a `.p12` export of a Developer ID Application certificate |
| `APPLE_CERTIFICATE_PASSWORD` | The password set when exporting it |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | The Apple ID used for notarisation |
| `APPLE_PASSWORD` | An app-specific password for that Apple ID, not the account password |
| `APPLE_TEAM_ID` | The ten-character team identifier |

To produce the first of those:

```bash
base64 -i certificate.p12 | pbcopy
```

Windows code signing is not wired up. Doing it properly now means either an EV certificate in a
hardware token, which cannot be used from a hosted runner, or Azure Trusted Signing, which needs an
organisation account. Neither is worth a half-measure: a certificate sitting in a repository secret
as a base64 `.pfx` is a signing key one compromised workflow away from signing malware with this
project's name on it. Windows installers therefore ship unsigned and SmartScreen says so, which is
the honest outcome.

## What unsigned costs the user

Both are documented in the release notes so nobody has to discover them:

- macOS refuses to open the application on first launch. Right-click → Open, or
  `xattr -dr com.apple.quarantine /Applications/AgentOS.app`.
- Windows SmartScreen warns that the publisher is unknown. More Info → Run anyway.

Neither is something a person should do to software they have not decided to trust, which is why
building from source stays a first-class path and takes one command.

## Verifying a download

Every CLI archive ships a `.sha256` generated on the machine that built it:

```bash
sha256sum -c agentos-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```
