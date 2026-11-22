Unicode true
RequestExecutionLevel admin
SilentInstall silent
Name "Uplink Network"
Icon "..\assets\icon\uplink.ico"
OutFile "${OUTPUT}"
!include FileFunc.nsh
Section
    ${GetParameters} $0
    System::Call 'kernel32::SetEnvironmentVariableW(w "UPLINK_NETWORK_ACTION", w r0) i.r1'
    StrCpy $3 2
    ClearErrors
    GetTempFileName $2 "$PROGRAMFILES64"
    IfErrors done
    Delete "$2"
    CreateDirectory "$2"
    IfErrors done
    SetOutPath "$2"
    File "network-action.ps1"
    File "remove-network.ps1"
    File "..\install-transport.ps1"
    SetOutPath "$2\transport\windows"
    File "..\transport\windows\Provision-DeviceProof.ps1"
    SetOutPath "$2"
    IfErrors finish
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$2\network-action.ps1"'
    Pop $0
    Pop $1
    StrCmp $0 "29" 0 +2
    StrCpy $3 29
    StrCmp $0 "0" 0 finish
    StrCpy $3 0
finish:
    SetOutPath "$TEMP"
    Delete "$2\.cache\zerotier-129\ZeroTierOne.msi"
    RMDir "$2\.cache\zerotier-129"
    RMDir "$2\.cache"
    Delete "$2\transport\windows\Provision-DeviceProof.ps1"
    RMDir "$2\transport\windows"
    RMDir "$2\transport"
    Delete "$2\install-transport.ps1"
    Delete "$2\network-action.ps1"
    Delete "$2\remove-network.ps1"
    RMDir "$2"
done:
    SetErrorLevel $3
SectionEnd