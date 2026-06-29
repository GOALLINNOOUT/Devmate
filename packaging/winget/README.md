# Windows Package Manager

Use this folder as the release checklist for submitting DevMate to winget after the first public GitHub Release exists.

## Package identity

- Package identifier: `ADELA.Devmate`
- Package name: `DevMate`
- Publisher: `ADELA`
- Homepage: `https://github.com/GOALLINNOOUT/Devmate`
- License: `MIT`

## Submission flow

1. Create a GitHub Release such as `v0.1.4`.
2. Confirm the Windows zip asset exists:
   `devmate-v0.1.4-x86_64-pc-windows-msvc.zip`
3. Copy the SHA256 from the generated `.sha256` asset.
4. Install wingetcreate:

```powershell
winget install Microsoft.WingetCreate
```

5. Create the manifest from the GitHub Release URL.

Important: `wingetcreate new` takes the installer URL as a positional argument. It does not accept `--id`, `--name`, `--publisher`, `--version`, or `--urls` on current versions.

```powershell
wingetcreate new https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.4/devmate-v0.1.4-x86_64-pc-windows-msvc.zip
```

6. Edit the generated manifest values if needed:

- Package identifier: `ADELA.Devmate`
- Package name: `DevMate`
- Publisher: `ADELA`
- Version: `0.1.4`
- Package URL: `https://github.com/GOALLINNOOUT/Devmate`
- License: `MIT`
- Short description: `Developer diagnostics CLI`
- Tags: `developer-tools`, `cli`, `diagnostics`, `jwt`, `git`
- Command: `devmate`

7. Validate locally:

```powershell
winget validate <path-to-generated-manifest-folder>
```

8. Submit to `microsoft/winget-pkgs`:

```powershell
wingetcreate submit <path-to-generated-manifest-folder>
```

This opens a pull request in the winget package repository. Microsoft validation checks the manifest and download URL before the PR can be merged.

## How To Know It Is Accepted

You are accepted on winget when the pull request in `microsoft/winget-pkgs` is merged.

After the merge, wait for the package index to update, then verify:

```powershell
winget source update
winget search ADELA.Devmate
winget show ADELA.Devmate
winget install ADELA.Devmate
```

If `winget search` finds `ADELA.Devmate`, users can install it with winget.

## Important Notes

- Winget approval is not the same as Microsoft code signing.
- Windows SmartScreen reputation may still warn users until the executable gains reputation or is code-signed.
- Keep the GitHub Release asset URL stable. Do not delete or replace the zip after submission.
- If the package changes, create a new GitHub Release and submit a new winget version.

Keep README install instructions pointing to GitHub Releases until the winget package is accepted.
