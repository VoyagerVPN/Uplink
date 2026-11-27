param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$ArgumentLine
)

$ErrorActionPreference='Stop'
$start=[Diagnostics.ProcessStartInfo]::new($Executable,$ArgumentLine)
$start.UseShellExecute=$true
$start.Verb='runas'
$start.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
$start.WorkingDirectory=[IO.Path]::GetDirectoryName($Executable)

try {
    $child=[Diagnostics.Process]::Start($start)
    try {
        $child.WaitForExit()
        exit $child.ExitCode
    } finally {$child.Dispose()}
} catch {
    # Отделяем ошибку запуска Windows от кода возврата установщика.
    $cause=$_.Exception.GetBaseException()
    if ($cause -is [ComponentModel.Win32Exception]) {
        exit (65536 + ($cause.NativeErrorCode -band 65535))
    }
    exit 65567
}
