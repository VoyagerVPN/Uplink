Unicode true
RequestExecutionLevel user
!include LogicLib.nsh
!include x64.nsh
!include FileFunc.nsh
Name "Uplink"
OutFile "${OUTPUT}"
Icon "..\assets\icon\uplink.ico"
UninstallIcon "..\assets\icon\uplink.ico"
SetCompressor /FINAL lzma
SetCompressorDictSize 16
AutoCloseWindow true
ShowInstDetails nevershow
ShowUninstDetails nevershow
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Uplink"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Uplink Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Uplink"
!define ARP "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define PROTO "Software\Classes\${PROTOCOL}"
!define RUN "Software\Microsoft\Windows\CurrentVersion\Run"
!define BASE "$PROGRAMFILES64\${PRODUCT}"
InstallDir ""
Var clientRoot
Var packageRoot
Var rootIndex
Var createdRoot
Var oldCommand
Var mutex
Var entry
Var search
Var marker
Var folder

!macro osinit
    SetShellVarContext all
    SetRegView 64
    ${IfNot} ${RunningX64}
        SetErrorLevel 24
        Quit
    ${EndIf}
!macroend

!macro lock
    System::Call 'kernel32::CreateMutexW(p 0,i 0,w "Local\${PRODUCT}-Package") p.r0 ?e'
    Pop $1
    StrCpy $mutex $0
    ${If} $0 == 0
    ${OrIf} $1 == 183
        SetErrorLevel 23
        Quit
    ${EndIf}
!macroend

!macro regularDirectory path onUnsafe
    System::Call 'kernel32::GetFileAttributesW(w "${path}") i.r8'
    ${If} $8 != -1
        IntOp $8 $8 & 0x400
        ${If} $8 != 0
            Goto ${onUnsafe}
        ${EndIf}
    ${EndIf}
!macroend

!getdllversion /packed "${BIN}\uplink.exe" ClientVersion_

; Сравниваем версии установленного клиента и пакета.
Function DetectInstallMode
    ReadRegStr $2 HKCU "Software\${PRODUCT}\Installer" "InstallLocation"
    ${If} $2 == ""
        ReadRegStr $2 HKCU "${ARP}" "InstallLocation"
    ${EndIf}
    ${If} $2 == ""
        ReadRegStr $2 HKLM "${ARP}" "InstallLocation"
    ${EndIf}
    Call CompareInstalledVersion
FunctionEnd

Function CompareInstalledVersion
    StrCmp $2 "" absent
    StrCpy $2 "$2\uplink.exe"
    IfFileExists "$2" compare absent
compare:
    ClearErrors
    GetDLLVersion "$2" $3 $4
    IfErrors repair
    IntCmpU $3 ${ClientVersion_HIGH} 0 update olderPackage
    IntCmpU $4 ${ClientVersion_LOW} repair update olderPackage
absent:
    Push "install"
    Return
repair:
    Push "reinstall"
    Return
update:
    Push "update"
    Return
olderPackage:
    Push "downgrade"
FunctionEnd

Function .onInit
    !insertmacro osinit
    StrCpy $clientRoot "$INSTDIR"
    ${If} $clientRoot == ""
        ReadRegStr $clientRoot HKCU "Software\${PRODUCT}\Installer" "PackageRoot"
        ${If} $clientRoot == ""
            ReadRegStr $clientRoot HKCU "${ARP}" "PackageRoot"
        ${EndIf}
        ${If} $clientRoot == ""
            ReadRegStr $clientRoot HKLM "${ARP}" "PackageRoot"
        ${EndIf}
        ${If} $clientRoot == ""
            StrCpy $clientRoot "${BASE}"
        ${EndIf}
    ${EndIf}
    StrCpy $packageRoot "${BASE}"
    ${GetRoot} "$packageRoot" $0
    ${If} $0 == ""
        SetErrorLevel 2
        Quit
    ${EndIf}
    StrCpy $INSTDIR "$packageRoot\packages\${TAG}"
    IfSilent backend
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "/oname=setup-ui.exe" "${BIN}\uplink-window.exe"
    Call DetectInstallMode
    Pop $1
    StrCpy $0 2
    ExecWait '"$PLUGINSDIR\setup-ui.exe" --setup-ui "$1" "$EXEPATH" "$clientRoot\packages\${TAG}" "${VERSION}" "${PRODUCT}"' $0
    SetErrorLevel $0
    Quit
backend:
    ${GetParameters} $2
    ClearErrors
    ${GetOptions} $2 "/UPLINKMACHINE" $3
    IfErrors caller
    UserInfo::GetAccountType
    Pop $1
    StrCmp $1 "Admin" elevated
    SetErrorLevel 2
    Quit
caller:
    ClearErrors
    ${GetOptions} $2 "/UPLINKUPDATE" $3
    IfErrors callerInstall
    Call DetectInstallMode
    Pop $1
    StrCmp $1 "update" callerUpdate
    SetErrorLevel 28
    Quit
callerUpdate:
    StrCpy $2 "/S /UPLINKMACHINE /UPLINKUPDATE"
    Goto callerReady
callerInstall:
    StrCpy $2 "/S /UPLINKMACHINE"
callerReady:
    UserInfo::GetAccountType
    Pop $1
    ClearErrors
    StrCmp $1 "Admin" 0 requestElevation
    ExecWait '"$EXEPATH" $2' $1
    Goto machineResult
requestElevation:
    ExecShellWait "runas" "$EXEPATH" "$2" SW_HIDE $1
machineResult:
    IfErrors 0 +2
    StrCpy $1 2
    StrCmp $1 0 0 clientDone
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "client-location.ps1"
    nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\client-location.ps1" Install "${PRODUCT}" "$clientRoot" "${TAG}"'
    Pop $1
    Pop $2
clientDone:
    SetErrorLevel $1
    Quit
elevated:
    !insertmacro lock
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/UPLINKUPDATE" $1
    IfErrors normalInstall
    ReadRegStr $2 HKLM "${ARP}" "InstallLocation"
    Call CompareInstalledVersion
    Pop $1
    StrCmp $1 "install" normalInstall
    StrCmp $1 "update" normalInstall
    ${If} $1 == "reinstall"
        ReadRegStr $2 HKLM "${ARP}" "InstallLocation"
        ${If} $2 == "$INSTDIR"
            SetErrorLevel 0
            Quit
        ${EndIf}
    ${EndIf}
    SetErrorLevel 28
    Quit
normalInstall:
FunctionEnd

Section
    ReadRegStr $0 HKLM "${PROTO}" ""
    ReadRegStr $oldCommand HKLM "${PROTO}\shell\open\command" ""
    ${If} $0 != ""
    ${AndIf} $0 != "URL:Uplink Invitation"
        SetErrorLevel 21
        Quit
    ${EndIf}
    !insertmacro regularDirectory "${BASE}\networks" failed
    !insertmacro regularDirectory "$packageRoot" failed
    !insertmacro regularDirectory "$packageRoot\packages" failed
    !insertmacro regularDirectory "$INSTDIR" failed
    StrLen $0 "$INSTDIR\uplink-network-service.exe"
    ${If} $0 >= 260
        SetErrorLevel 2
        Quit
    ${EndIf}
    ClearErrors
    ReadRegDWORD $createdRoot HKLM "${ARP}\Roots" "$packageRoot"
    ${If} ${Errors}
        StrCpy $createdRoot 1
        IfFileExists "$packageRoot\." 0 +2
        StrCpy $createdRoot 0
    ${EndIf}
    !macro checkFile path
        IfFileExists "${path}" 0 +5
        ClearErrors
        FileOpen $0 "${path}" a
        IfErrors locked
        FileClose $0
    !macroend
    !insertmacro checkFile "$INSTDIR\uplink.exe"
    !insertmacro checkFile "$INSTDIR\uplink-window.exe"
    !insertmacro checkFile "$INSTDIR\uplinkctl.exe"
    ClearErrors
    SetOutPath "$INSTDIR"
    ; Содержимое каждой версии адресуется хешем всего пакета.
    SetOverwrite on
    File "${BIN}\uplink.exe"
    File "${BIN}\uplink-window.exe"
    File "${BIN}\uplinkctl.exe"
    File "..\assets\fonts\Inter-LICENSE.txt"
    File "${BIN}\uplink-network-service.exe"
    File "${ENGINE}\uplink-engine.exe"
    File "${ENGINE}\wintun.dll"
    File "${ENGINE}\Packet.dll"
    File "${ENGINE}\WinDivert64.sys"
    File "${ENGINE}\WinDivert.dll"
    File "${ENGINE}\WinDivert-LICENSE.txt"
    File "manage-service.ps1"
    File "EasyTier-LICENSE.txt"
    SetOutPath "$INSTDIR"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    IfErrors failed
    FileOpen $0 "$INSTDIR\package.id" w
    FileWrite $0 "${PRODUCT}|${TAG}"
    FileClose $0
    IfErrors failed
    nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\manage-service.ps1" Install "${PRODUCT}" "${TAG}"'
    Pop $0
    Pop $2
    StrCmp $0 0 +2
    Goto failed
    CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\uplink.exe"
    WriteRegStr HKLM "${PROTO}" "" "URL:Uplink Invitation"
    WriteRegStr HKLM "${PROTO}" "URL Protocol" ""
    WriteRegStr HKLM "${PROTO}\DefaultIcon" "" '"$INSTDIR\uplink.exe",0'
    WriteRegStr HKLM "${PROTO}\shell\open\command" "" '"$INSTDIR\uplink.exe" "%1"'
    WriteRegStr HKLM "${ARP}" "DisplayName" "${PRODUCT}"
    WriteRegStr HKLM "${ARP}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "${ARP}" "Publisher" "Uplink"
    WriteRegStr HKLM "${ARP}" "DisplayIcon" "$INSTDIR\uplink.exe"
    WriteRegStr HKLM "${ARP}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "${ARP}" "PackageRoot" "$packageRoot"
    WriteRegDWORD HKLM "${ARP}\Roots" "$packageRoot" $createdRoot
    WriteRegStr HKLM "${ARP}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKLM "${ARP}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKLM "${ARP}" "NoModify" 1
    WriteRegDWORD HKLM "${ARP}" "NoRepair" 1
    IfErrors failed
    System::Call 'shell32::SHChangeNotify(i 0x08000000,i 0,p 0,p 0)'
    SetErrorLevel 0
    Goto done
locked:
    SetErrorLevel 22
    Quit
failed:
    SetErrorLevel 2
done:
SectionEnd

Var machineOnly
Var userPurge

Function un.userCleanup
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "client-location.ps1"
    nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\client-location.ps1" Remove "${PRODUCT}"'
    Pop $0
    Pop $2
    StrCmp $0 0 +3
    SetErrorLevel 28
    Quit
    File "/oname=cleanup-user.ps1" "cleanup-user.ps1"
    nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\cleanup-user.ps1" "${PRODUCT}" $userPurge'
    Pop $0
    Pop $2
    StrCmp $0 0 +3
    SetErrorLevel 28
    Quit
FunctionEnd

Function un.onInit
    !insertmacro osinit
    ReadRegStr $0 HKLM "${ARP}" "InstallLocation"
    ${If} $0 != "$INSTDIR"
        SetErrorLevel 2
        Quit
    ${EndIf}
    IfSilent backend
    InitPluginsDir
    CopyFiles /SILENT "$INSTDIR\uplink-window.exe" "$PLUGINSDIR\setup-ui.exe"
    StrCpy $0 2
    ExecWait '"$PLUGINSDIR\setup-ui.exe" --setup-ui remove "$EXEPATH" "$INSTDIR" "${VERSION}" "${PRODUCT}"' $0
    SetErrorLevel $0
    Quit
backend:
    ${GetParameters} $2
    StrCpy $userPurge ""
    ClearErrors
    ${GetOptions} $2 "/PURGEDATA" $3
    IfErrors +2
    StrCpy $userPurge "-Purge"
    StrCpy $machineOnly 0
    ClearErrors
    ${GetOptions} $2 "/UPLINKMACHINE" $3
    IfErrors +2
    StrCpy $machineOnly 1
    UserInfo::GetAccountType
    Pop $1
    StrCmp $1 "Admin" elevated
    ClearErrors
    ExecShellWait "runas" "$INSTDIR\Uninstall.exe" '/S /UPLINKMACHINE _?=$INSTDIR' SW_HIDE $1
    IfErrors 0 +2
    StrCpy $1 2
    StrCmp $1 0 0 callerDone
    Call un.userCleanup
    StrCpy $1 0
callerDone:
    SetErrorLevel $1
    Quit
elevated:
    !insertmacro lock
FunctionEnd

Function un.package
    ; Удаляем только перечисленные файлы из помеченных каталогов пакета.
    StrCpy $folder "$packageRoot\packages\$entry"
    System::Call 'kernel32::GetFileAttributesW(w "$folder") i.r0'
    IntOp $0 $0 & 0x400
    StrCmp $0 0 +2
    Return
    ClearErrors
    FileOpen $0 "$folder\package.id" r
    IfErrors skip
    FileRead $0 $marker
    FileClose $0
    StrCmp $marker "${PRODUCT}|$entry" 0 skip
    ${If} $1 == "check"
        !macro unlocked filename
            ClearErrors
            IfFileExists "$folder\${filename}" 0 +4
            FileOpen $0 "$folder\${filename}" a
            IfErrors locked
            FileClose $0
        !macroend
        !insertmacro unlocked uplink.exe
        !insertmacro unlocked uplink-window.exe
        !insertmacro unlocked uplinkctl.exe
        !insertmacro unlocked uplink-network-service.exe
        !insertmacro unlocked uplink-engine.exe
        Return
    ${EndIf}
    !macro removeOwned file
        IfFileExists "$folder\${file}" 0 +3
        Delete "$folder\${file}"
        IfFileExists "$folder\${file}" locked
    !macroend
    !insertmacro removeOwned uplink.exe
    !insertmacro removeOwned "uplink-window.exe"
    !insertmacro removeOwned "uplinkctl.exe"
    !insertmacro removeOwned "uplink-network-service.exe"
    !insertmacro removeOwned "uplink-engine.exe"
    !insertmacro removeOwned "wintun.dll"
    !insertmacro removeOwned "Packet.dll"
    !insertmacro removeOwned "WinDivert64.sys"
    !insertmacro removeOwned "WinDivert.dll"
    !insertmacro removeOwned "WinDivert-LICENSE.txt"
    !insertmacro removeOwned "manage-service.ps1"
    !insertmacro removeOwned "EasyTier-LICENSE.txt"
    !insertmacro removeOwned "Uninstall.exe"
    !insertmacro removeOwned "Inter-LICENSE.txt"
    !insertmacro removeOwned "package.id"
    RMDir "$folder"
skip:
    Return
locked:
    FindClose $search
    SetErrorLevel 22
    Quit
FunctionEnd

Function un.walk
    FindFirst $search $entry "$packageRoot\packages\*"
loop:
    StrCmp $entry "" finish
    StrCmp $entry "." next
    StrCmp $entry ".." next
    Call un.package
next:
    FindNext $search $entry
    Goto loop
finish:
    FindClose $search
FunctionEnd

Function un.roots
    StrCpy $rootIndex 0
again:
    EnumRegValue $packageRoot HKLM "${ARP}\Roots" $rootIndex
    StrCmp $packageRoot "" done
    !insertmacro regularDirectory "$packageRoot" unsafe
    !insertmacro regularDirectory "$packageRoot\packages" unsafe
    Call un.walk
    ${If} $1 == "remove"
        RMDir "$packageRoot\packages"
        ReadRegDWORD $0 HKLM "${ARP}\Roots" "$packageRoot"
        ${If} $0 == 1
            RMDir "$packageRoot"
        ${EndIf}
    ${EndIf}
    IntOp $rootIndex $rootIndex + 1
    Goto again
unsafe:
    SetErrorLevel 2
    Quit
done:
FunctionEnd

Section Uninstall
    ReadRegStr $0 HKLM "${ARP}" "InstallLocation"
    ; Старый деинсталлятор не снимает регистрацию новой версии.
    ${If} $0 != "$INSTDIR"
        SetErrorLevel 2
        Quit
    ${EndIf}
    nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\manage-service.ps1" Remove "${PRODUCT}" "${TAG}"'
    Pop $0
    Pop $2
    StrCmp $0 0 +3
    SetErrorLevel 28
    Quit
    StrCpy $1 "check"
    Call un.roots
    StrCpy $1 "remove"
    Call un.roots
    RMDir "${BASE}\networks"
    RMDir "${BASE}"
    ReadRegStr $0 HKLM "${PROTO}\shell\open\command" ""
    ${If} $0 == '"$INSTDIR\uplink.exe" "%1"'
        DeleteRegKey HKLM "${PROTO}"
    ${EndIf}
    Delete "$SMPROGRAMS\${PRODUCT}.lnk"
    DeleteRegKey HKLM "${ARP}"
    System::Call 'shell32::SHChangeNotify(i 0x08000000,i 0,p 0,p 0)'
    StrCmp $machineOnly 1 +2
    Call un.userCleanup
    SetErrorLevel 0
SectionEnd
