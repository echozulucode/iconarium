# Quick Start

AI shall not modify this file.

## Manual Release

```
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\iconarium-updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Read-Host -Prompt "Enter Secret Key" -MaskInput
just build
```
