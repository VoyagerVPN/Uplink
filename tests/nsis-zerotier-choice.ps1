$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo=Split-Path $PSScriptRoot
$work=New-Item -Type Directory -Path (Join-Path $repo ('.cache/nsis/choice-'+[guid]::NewGuid().ToString('N')))
$env:UPLINK_PREFS_DIR=$work.FullName
$env:ProgramData=$work.FullName
$homeDir=New-Item -Type Directory -Path (Join-Path $work.FullName 'ZeroTier/One') -Force
$bin=Join-Path $repo '.cache/target/perf/uplink-window.exe'
$receipt=Join-Path $homeDir.FullName '.uplink-owner'
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
foreach ($case in 'absent','existing','owned','another-user') {
    switch($case) {
        'existing' {New-Item -Type File -Path (Join-Path $homeDir.FullName 'zerotier-one_x64.exe') | Out-Null}
        'owned' {[IO.File]::WriteAllText($receipt,$sid)}
        'another-user' {[IO.File]::WriteAllText($receipt,'S-1-5-12345')}
    }
    $p=Start-Process $bin -ArgumentList '--setup-ui remove unused.exe unused\packages\test 0.1.0 Uplink' -WindowStyle Hidden -PassThru
    try {
        $deadline=[DateTime]::UtcNow.AddSeconds(10)
        while ($p.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline) {Start-Sleep -Milliseconds 100;$p.Refresh()}
        $window=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
        $tree=[Windows.Automation.TreeScope]::Descendants
        $node=New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty,'Удалить ZeroTier')
        $box=$window.FindFirst($tree,$node)
        if ($case -eq 'absent') {if ($box) {throw 'Offered absent ZeroTier'}}
        else {
            $state=$box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
            if (($state -eq 'On') -ne ($case -eq 'owned')) {throw "Incorrect ZeroTier default: $case"}
        }
        "$case OK"
    } finally {$null=$p.CloseMainWindow();if (!$p.WaitForExit(4000)) {$p.Kill()}}
}
