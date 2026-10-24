# Adds or removes the Iconarium install folder in the current user's PATH, so
# `iconarium-cli` works in any new terminal. Run by the installer hooks
# (installer-hooks.nsh); safe to run repeatedly.
#
# Edits HKCU\Environment\Path directly, preserving REG_EXPAND_SZ and unexpanded entries
# such as %USERPROFILE%\...: [Environment]::SetEnvironmentVariable would rewrite the value
# as REG_SZ and break those. The installer broadcasts WM_SETTINGCHANGE afterwards.
param(
    [Parameter(Mandatory = $true)][ValidateSet('add', 'remove')][string]$Action,
    [Parameter(Mandatory = $true)][string]$Dir
)
$ErrorActionPreference = 'Stop'
$key = 'HKCU:\Environment'
$norm = { param($p) $p.Trim().TrimEnd('\') }
$target = & $norm $Dir

$item = Get-Item -Path $key
$current = [string]$item.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$kind = if ($item.GetValueNames() -contains 'Path') { $item.GetValueKind('Path') } else { 'ExpandString' }
$entries = $current -split ';'
$present = @($entries | Where-Object { $_.Trim() -ne '' -and (& $norm $_) -ieq $target }).Count -gt 0

if ($Action -eq 'add') {
    if ($present) { exit 0 }
    # Append, leaving the existing text exactly as it was.
    $trimmed = $current.TrimEnd(';')
    $value = if ($trimmed.Trim() -eq '') { $Dir.TrimEnd('\') } else { $trimmed + ';' + $Dir.TrimEnd('\') }
} else {
    if (-not $present) { exit 0 }
    # Drop only our entry; every other entry (even empty ones) is kept as is.
    $value = (@($entries | Where-Object { $_.Trim() -eq '' -or (& $norm $_) -ine $target })) -join ';'
}

if ($kind -ne 'String') { $kind = 'ExpandString' }
New-ItemProperty -Path $key -Name 'Path' -PropertyType $kind -Value $value -Force | Out-Null
