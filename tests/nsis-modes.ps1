param(
    [Parameter(Mandatory)][string]$Setup,
    [Parameter(Mandatory)][ValidatePattern('^Uplink-QA-[a-z0-9]+$')][string]$Product
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = (Resolve-Path $Setup).Path
if (![IO.Path]::GetFileName($exe).StartsWith("$Product-Setup", [StringComparison]::OrdinalIgnoreCase)) {throw 'Expected a matching QA package'}
$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"
$protocol = "HKCU:\Software\Classes\$Product"
$root = Join-Path $env:LOCALAPPDATA "Programs/$Product"
if ((Test-Path $key) -or (Test-Path $protocol) -or (Test-Path $root)) {throw 'Test namespace already in use'}
$work = New-Item -Type Directory -Path (Join-Path $repo ('.cache/nsis/modes-' + [Guid]::NewGuid().ToString('N')))
$prior = Join-Path $work.FullName 'uplink.exe'
$env:UPLINK_PREFS_DIR = $work.FullName
$pref = New-Item -ItemType Directory -Path (Join-Path $work.FullName 'Uplink')
[IO.File]::WriteAllText((Join-Path $pref.FullName 'prefs.json'),'{"language":"ru"}')

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UplinkTestTray {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    static extern IntPtr FindWindowEx(IntPtr parent,IntPtr after,string cls,string title);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
    public static bool Ready(uint pid) {
        IntPtr w=IntPtr.Zero;
        while ((w=FindWindowEx(IntPtr.Zero,w,null,"Uplink Tray"))!=IntPtr.Zero) {
            uint found; GetWindowThreadProcessId(w,out found);
            if (found==pid) return true;
        }
        return false;
    }
}
"@
function Wait-Control($ui,[string]$label) {
    $query=New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty,$label)
    $deadline=[DateTime]::UtcNow.AddSeconds(20)
    do {
        $control=$ui.FindFirst([Windows.Automation.TreeScope]::Descendants,$query)
        if ($control) {return $control}
        Start-Sleep -Milliseconds 120
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Missing control: $label"
}
function Invoke-Control($ui,[string]$label) {
    (Wait-Control $ui $label).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Assert-Mode([string]$label, [string]$title, [switch]$Apply,[switch]$Uninstall,[switch]$Purge,$Client) {
    if ($Uninstall) {
        $dir=Get-ItemPropertyValue $key InstallLocation
        $copy=Join-Path $work.FullName 'uninstall-ui.exe'
        Copy-Item (Join-Path $dir 'Uninstall.exe') $copy -Force
        $process=Start-Process -FilePath $copy -ArgumentList ('_?='+$dir) -WindowStyle Hidden -PassThru
    } else {$process = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru}
    $window = $null
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            $child = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($process.Id) AND Name='setup-ui.exe'"
            if ($child) {
                $window = Get-Process -Id $child.ProcessId
                if ($window.MainWindowHandle -ne [IntPtr]::Zero -and $window.MainWindowTitle -ceq 'Установка Uplink') {break}
            }
            Start-Sleep -Milliseconds 120
        } while (!$process.HasExited -and [DateTime]::UtcNow -lt $deadline)
        if (!$window -or $window.MainWindowHandle -eq [IntPtr]::Zero -or $window.MainWindowTitle -cne 'Установка Uplink') {throw "Setup window missing: $label"}
        $ui = [Windows.Automation.AutomationElement]::FromHandle($window.MainWindowHandle)
        $subtree = [Windows.Automation.TreeScope]::Descendants
        $all = [Windows.Automation.Condition]::TrueCondition
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            $window.Refresh()
            $ui = [Windows.Automation.AutomationElement]::FromHandle($window.MainWindowHandle)
            $nodes = @($ui.FindAll($subtree,$all))
            $button = $nodes | Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.Name -ceq $label} | Select-Object -First 1
            if ($button -and $title -cin @($nodes | ForEach-Object {$_.Current.Name})) {break}
            Start-Sleep -Milliseconds 120
        } while ([DateTime]::UtcNow -lt $deadline)
        if (!$button -or $title -cnotin @($nodes | ForEach-Object {$_.Current.Name})) {
            throw "Expected '$title' / '$label'; visible: $(@($nodes | ForEach-Object {$_.Current.Name}) -join ', ')"
        }
        if ($Apply) {
            if ($Uninstall) {
                if ('Что удалить вместе с приложением?' -cin @($nodes | ForEach-Object {$_.Current.Name})) {throw 'Unexpected uninstall subtitle'}
                $box=Wait-Control $ui 'Удалить настройки и сохранённые сети'
                $toggle=$box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
                if ($toggle.Current.ToggleState -ne 'Off') {throw 'Profile deletion selected by default'}
                if ($Purge) {$toggle.Toggle()}
            }
            $button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
            if ($Client) {
                $null=Wait-Control $ui 'Закрыть Uplink?'
                Invoke-Control $ui 'Отмена'
                if ($Client.HasExited) {throw 'Cancellation closed the client'}
                Invoke-Control $ui $label
                Invoke-Control $ui 'Закрыть и продолжить'
            }
            $done=if ($Uninstall) {'Uplink удалён'} else {'Uplink готов'}
            $null=Wait-Control $ui $done
            if ($Client -and !$Client.HasExited) {throw 'Tray is still running'}
            if ($ui.FindAll($subtree,$all) | Where-Object {$_.Current.Name -ceq 'Настроить транспорт'}) {throw 'Setup is offering network configuration'}
        }
        Write-Output "$title / $label OK"
    } catch {Write-Host "UI test failed: $_ at $($_.InvocationInfo.ScriptLineNumber)";throw} finally {
        if ($window -and !$window.HasExited) { $null=$window.CloseMainWindow() }
        if (!$process.WaitForExit(5000)) {Write-Warning 'Closing the isolated test window'; if ($window -and !$window.HasExited) {$window.Kill()}; if (!$process.WaitForExit(5000)) {$process.Kill()} }
    }
}

function Remove-QAClient {
    if (Test-Path $key) {
        $dir=Get-ItemPropertyValue $key InstallLocation
        if ((Split-Path (Split-Path $dir)) -ne $root) {throw 'Unexpected QA root'}
        $uninstall=Join-Path $work.FullName 'uninstall.exe'
        Copy-Item -LiteralPath (Join-Path $dir 'Uninstall.exe') -Destination $uninstall -Force
        $p=Start-Process -FilePath $uninstall -ArgumentList ('/S _?='+$dir) -Wait -PassThru -WindowStyle Hidden
        if ($p.ExitCode) {throw 'QA uninstall failed'}
    }
}

try {
    # Старый обработчик сохранился, но сам клиент уже удалён.
    New-Item -Path "$protocol\shell\open\command" -Force | Out-Null
    Set-Item $protocol -Value 'URL:Uplink Invitation'
    Set-Item "$protocol\shell\open\command" -Value ('"'+$prior+'" "%1"')
    Assert-Mode 'Установить' 'Установить Uplink' -Apply
    $installed=Join-Path (Get-ItemPropertyValue $key InstallLocation) 'uplink.exe'
    if (!(Test-Path -LiteralPath $installed)) {throw 'First install did not create the client'}
    Remove-QAClient
    New-Item -Path "$protocol\shell\open\command" -Force | Out-Null
    Set-Item $protocol -Value 'URL:Uplink Invitation'
    Set-Item "$protocol\shell\open\command" -Value ('"'+$prior+'" "%1"')

    # PE с версией прежнего клиента, зарегистрированный способом install-user.ps1.
    $fixture = @"
Unicode true
Name Uplink
OutFile "$prior"
RequestExecutionLevel user
VIProductVersion "0.0.9.0"
VIAddVersionKey "ProductName" "uplink"
VIAddVersionKey "FileDescription" "Uplink QA"
VIAddVersionKey "FileVersion" "0.0.9"
VIAddVersionKey "LegalCopyright" "Uplink QA"
Section
SectionEnd
"@
    $source=Join-Path $work.FullName 'prior.nsi'
    [IO.File]::WriteAllText($source,$fixture.Replace('0.0.9','1.0.0'))
    & (Join-Path $repo '.cache/nsis/nsis-3.13/makensis.exe') /INPUTCHARSET UTF8 /V2 $source
    if ($LASTEXITCODE) {throw 'Downgrade fixture failed'}
    Assert-Mode 'Откатить' 'Откатить Uplink'
    [IO.File]::WriteAllText($source,$fixture)
    & (Join-Path $repo '.cache/nsis/nsis-3.13/makensis.exe') /INPUTCHARSET UTF8 /V2 $source
    if ($LASTEXITCODE) {throw 'Previous-version fixture failed'}
    Assert-Mode 'Обновить' 'Обновить Uplink' -Apply
    $dir=Get-ItemPropertyValue $key InstallLocation
    if (!(Test-Path -LiteralPath (Join-Path $dir 'uplink.exe'))) {throw 'Client missing after update'}
    # Версия для сравнения находится в EXE. Данные Windows о пакете могут устареть.
    Set-ItemProperty $key DisplayVersion '0.0.1'
    Assert-Mode 'Переустановить' 'Переустановить Uplink' -Apply
    if ((Get-ItemPropertyValue $key InstallLocation) -ne $dir) {throw 'Reinstall switched package'}
    $data=Join-Path ([Environment]::GetFolderPath('ApplicationData')) $Product
    if (Test-Path -LiteralPath $data) {throw 'Test data namespace in use'}
    New-Item -Type Directory -Path $data | Out-Null
    $prefs=Join-Path $data 'prefs.json'
    [IO.File]::WriteAllText($prefs,'{"rooms":["qa"]}')
    [IO.File]::WriteAllText((Join-Path $data 'prefs.lock'),'')
    $client=Start-Process (Join-Path $dir 'uplink.exe') -ArgumentList '--background' -WindowStyle Hidden -PassThru
    try {
        $deadline=[DateTime]::UtcNow.AddSeconds(15)
        while (![UplinkTestTray]::Ready($client.Id)) {
            if ($client.HasExited -or [DateTime]::UtcNow -gt $deadline) {throw 'Test tray did not start'}
            Start-Sleep -Milliseconds 100
        }
        Assert-Mode 'Удалить' 'Удалить Uplink?' -Uninstall -Apply -Client $client
    } finally {if (!$client.HasExited) {$client.Kill();$client.WaitForExit()}}
    if ([IO.File]::ReadAllText($prefs) -cne '{"rooms":["qa"]}' -or (Test-Path (Join-Path $data 'prefs.lock'))) {throw 'Default removal changed saved networks or retained runtime files'}
    Assert-Mode 'Установить' 'Установить Uplink' -Apply
    Assert-Mode 'Удалить' 'Удалить Uplink?' -Uninstall -Apply -Purge
    if (Test-Path $data) {throw 'Profile survived full uninstall'}
} finally {
    Remove-QAClient
    if (Test-Path $protocol) {
        foreach ($k in "$protocol\shell\open\command","$protocol\shell\open","$protocol\shell",$protocol) {Remove-Item -LiteralPath $k}
    }
}
