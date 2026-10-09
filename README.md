# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Releasing

Releases are built and published by [`.github/workflows/release.yml`](.github/workflows/release.yml).
Pushing a tag that starts with `v` builds the NSIS and MSI installers for Windows, signs them
for the in-app updater, and publishes a GitHub Release with a `latest.json` manifest.

```sh
git tag v0.1.1
git push origin v0.1.1
```

The version in `src-tauri/tauri.conf.json` is overwritten from the tag during the build, so the
committed version is just the local dev default.

### One-time setup

1. **Public repo** — the updater downloads `latest.json` and the installer over plain HTTPS, so
   the repository must be public (a private repo returns 404 for release assets).
2. **Signing key** — the updater refuses unsigned bundles. Generate a password-protected
   keypair (the CLI prompts for the password):

   ```sh
   pnpm tauri signer generate -w ~/.tauri/clipper23.key
   ```

   - Put the **public** key (`clipper23.key.pub`) in `plugins.updater.pubkey` in
     `src-tauri/tauri.conf.json`.
   - Add two repository secrets (Settings → Secrets and variables → Actions → New repository
     secret): `TAURI_SIGNING_PRIVATE_KEY` (the private key) and
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the password you chose).
   - **Back up both the private key and its password somewhere private, and never commit
     them.** If either is lost you must ship a new keypair, and apps already installed (which
     trust the old public key) can no longer auto-update — users would have to reinstall
     manually.

### How updates reach users

On launch (and from the **Updates** panel in Settings) the app checks the release manifest. When
a newer version exists, Settings shows a banner with an **Update & restart** button that
downloads, installs (Windows passive mode) and relaunches the app.

