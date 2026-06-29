# Windows Package Manager

Use this folder as the release checklist for submitting DevMate to winget after the first public GitHub Release exists.

## Package identity

- Package identifier: `ADELA.Devmate`
- Package name: `DevMate`
- Publisher: `ADELA`
- Homepage: `https://github.com/GOALLINNOOUT/Devmate`
- License: `MIT`

## Submission flow

1. Create a GitHub Release such as `v0.1.0`.
2. Confirm the Windows zip asset exists:
   `devmate-v0.1.0-x86_64-pc-windows-msvc.zip`
3. Copy the SHA256 from the generated `.sha256` asset.
4. Create or update the winget manifest with `wingetcreate`.
5. Validate locally with `winget validate`.
6. Submit to `microsoft/winget-pkgs`.

Keep README install instructions pointing to GitHub Releases until the winget package is accepted.
