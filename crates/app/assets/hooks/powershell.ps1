# Ngambang: tell the island when a long command finishes.
# Add to your PowerShell profile:
#   ngambang hook powershell | Out-String | Invoke-Expression
$global:__ngbLastId = (Get-History -Count 1).Id
$global:__ngbIgnore = {{IGNORE}}
$global:__ngbPrompt = $function:prompt
function global:prompt {
    $ok = $?
    $code = $global:LASTEXITCODE
    $h = Get-History -Count 1
    if ($h -and $h.Id -ne $global:__ngbLastId) {
        $global:__ngbLastId = $h.Id
        $secs = [int]($h.EndExecutionTime - $h.StartExecutionTime).TotalSeconds
        $first = ($h.CommandLine.Trim() -split '\s+')[0]
        if ($secs -ge {{THRESHOLD}} -and $global:__ngbIgnore -notcontains $first) {
            if ($ok) {
                ngambang done $h.CommandLine --ok --duration $secs --quiet
            } else {
                $c = if ($code) { $code } else { 1 }
                ngambang done $h.CommandLine --fail --code $c --duration $secs --quiet
            }
        }
    }
    $global:LASTEXITCODE = $code
    & $global:__ngbPrompt
}
