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
VIAddVersionKey "FileDescription" "Uplink Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Uplink"
!define ARP "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define PROTO "Software\Classes\${PROTOCOL}"
!define RUN "Software\Microsoft\Windows\CurrentVersion\Run"
!define BASE "$LOCALAPPDATA\Programs\${PRODUCT}"
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
    SetShellVarContext current
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

; Сначала проверяем путь из записи установленного пакета, затем прежнюю регистрацию install-user.ps1.
Function DetectInstallMode
    ReadRegStr $2 HKCU "${ARP}" "InstallLocation"
    StrCmp $2 "" legacy
    StrCpy $2 "$2\uplink.exe"
    IfFileExists "$2" compare
legacy:
    ReadRegStr $0 HKCR "${PROTOCOL}" ""
    StrCmp $0 "URL:Uplink Invitation" 0 absent
    ReadRegStr $0 HKCR "${PROTOCOL}\shell\open\command" ""
    StrCpy $2 $0 1
    StrCmp $2 '"' 0 absent
    StrCpy $2 $0 6 -6
    StrCmp $2 '" "%1"' 0 absent
    StrCpy $2 $0 -6 1
    ${GetFileName} "$2" $0
    StrCmp $0 "uplink.exe" 0 absent
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
    ; /D= имеет приоритет над последним выбранным каталогом и каталогом по умолчанию.
    ${If} $INSTDIR == ""
        ReadRegStr $INSTDIR HKCU "${ARP}" "PackageRoot"
        ${If} $INSTDIR == ""
            StrCpy $INSTDIR "${BASE}"
        ${EndIf}
    ${EndIf}
    StrCpy $packageRoot "$INSTDIR"
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
    ExecWait '"$PLUGINSDIR\setup-ui.exe" --setup-ui "$1" "$EXEPATH" "$INSTDIR" "${VERSION}" "${PRODUCT}"' $0
    SetErrorLevel $0
    Quit
backend:
    !insertmacro lock
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/UPLINKUPDATE" $1
    IfErrors normalInstall
    Call DetectInstallMode
    Pop $1
    StrCmp $1 "update" normalInstall
    SetErrorLevel 28
    Quit
normalInstall:
FunctionEnd

Section
    ReadRegStr $0 HKCR "${PROTOCOL}" ""
    ReadRegStr $oldCommand HKCR "${PROTOCOL}\shell\open\command" ""
    ${If} $0 != ""
    ${AndIf} $0 != "URL:Uplink Invitation"
        SetErrorLevel 21
        Quit
    ${EndIf}
    !insertmacro regularDirectory "$packageRoot" failed
    !insertmacro regularDirectory "$packageRoot\packages" failed
    !insertmacro regularDirectory "$INSTDIR" failed
    !insertmacro regularDirectory "$INSTDIR\transport" failed
    !insertmacro regularDirectory "$INSTDIR\transport\windows" failed
    StrLen $0 "$INSTDIR\transport\windows\Provision-DeviceProof.ps1"
    ${If} $0 >= 260
        SetErrorLevel 2
        Quit
    ${EndIf}
    ClearErrors
    ReadRegDWORD $createdRoot HKCU "${ARP}\Roots" "$packageRoot"
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
    File "${NETWORK_HELPER}"
    SetOutPath "$INSTDIR"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    IfErrors failed
    FileOpen $0 "$INSTDIR\package.id" w
    FileWrite $0 "${PRODUCT}|${TAG}"
    FileClose $0
    IfErrors failed
    ReadRegStr $1 HKCU "${RUN}" "${PRODUCT}"
    ClearErrors
    StrCpy $0 $oldCommand -5
    ${If} $oldCommand != ""
    ${AndIf} $1 == '$0 --background'
        WriteRegStr HKCU "${RUN}" "${PRODUCT}" '"$INSTDIR\uplink.exe" --background'
    ${EndIf}
    CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\uplink.exe"
    WriteRegStr HKCU "${PROTO}" "" "URL:Uplink Invitation"
    WriteRegStr HKCU "${PROTO}" "URL Protocol" ""
    WriteRegStr HKCU "${PROTO}\DefaultIcon" "" '"$INSTDIR\uplink.exe",0'
    WriteRegStr HKCU "${PROTO}\shell\open\command" "" '"$INSTDIR\uplink.exe" "%1"'
    WriteRegStr HKCU "${ARP}" "DisplayName" "${PRODUCT}"
    WriteRegStr HKCU "${ARP}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${ARP}" "Publisher" "Uplink"
    WriteRegStr HKCU "${ARP}" "DisplayIcon" "$INSTDIR\uplink.exe"
    WriteRegStr HKCU "${ARP}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${ARP}" "PackageRoot" "$packageRoot"
    WriteRegDWORD HKCU "${ARP}\Roots" "$packageRoot" $createdRoot
    WriteRegStr HKCU "${ARP}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "${ARP}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKCU "${ARP}" "NoModify" 1
    WriteRegDWORD HKCU "${ARP}" "NoRepair" 1
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

Function un.onInit
    !insertmacro osinit
    ReadRegStr $0 HKCU "${ARP}" "InstallLocation"
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
    !insertmacro regularDirectory "$folder\transport" locked
    !insertmacro regularDirectory "$folder\transport\windows" locked
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
        !insertmacro unlocked UplinkNetwork.exe
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
    !insertmacro removeOwned "UplinkNetwork.exe"
    !insertmacro removeOwned "Uninstall.exe"
    !insertmacro removeOwned "Inter-LICENSE.txt"
    !insertmacro removeOwned "install-transport.ps1"
    !insertmacro removeOwned "transport\windows\Provision-DeviceProof.ps1"
    RMDir "$folder\transport\windows"
    RMDir "$folder\transport"
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
    EnumRegValue $packageRoot HKCU "${ARP}\Roots" $rootIndex
    StrCmp $packageRoot "" done
    !insertmacro regularDirectory "$packageRoot" unsafe
    !insertmacro regularDirectory "$packageRoot\packages" unsafe
    Call un.walk
    ${If} $1 == "remove"
        RMDir "$packageRoot\packages"
        ReadRegDWORD $0 HKCU "${ARP}\Roots" "$packageRoot"
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
    ReadRegStr $0 HKCU "${ARP}" "InstallLocation"
    ; Старый деинсталлятор не снимает регистрацию новой версии.
    ${If} $0 != "$INSTDIR"
        SetErrorLevel 2
        Quit
    ${EndIf}
    StrCpy $1 "check"
    Call un.roots
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "/oname=cleanup-user.ps1" "cleanup-user.ps1"
    ${GetParameters} $3
    ClearErrors
    ${GetOptions} "$3" "/PURGEDATA" $0
    StrCpy $4 ""
    IfErrors +2
    StrCpy $4 "-Purge"
    ClearErrors
    ${GetOptions} "$3" "/REMOVEZEROTIER" $0
    StrCpy $5 ""
    IfErrors +2
    StrCpy $5 "-RemoveZeroTier"
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\cleanup-user.ps1" -Product "${PRODUCT}" -Helper "$INSTDIR\UplinkNetwork.exe" $4 $5'
    Pop $0
    Pop $2
    StrCmp $0 "29" 0 +3
    SetErrorLevel 29
    Quit
    StrCmp $0 "0" +3
    SetErrorLevel 28
    Quit
    StrCpy $1 "remove"
    Call un.roots
    RMDir "${BASE}"
    ReadRegStr $0 HKCU "${PROTO}\shell\open\command" ""
    ${If} $0 == '"$INSTDIR\uplink.exe" "%1"'
        DeleteRegKey HKCU "${PROTO}"
    ${EndIf}
    ReadRegStr $0 HKCU "${RUN}" "${PRODUCT}"
    ${If} $0 == '"$INSTDIR\uplink.exe" --background'
        DeleteRegValue HKCU "${RUN}" "${PRODUCT}"
    ${EndIf}
    Delete "$SMPROGRAMS\${PRODUCT}.lnk"
    DeleteRegKey HKCU "${ARP}"
    System::Call 'shell32::SHChangeNotify(i 0x08000000,i 0,p 0,p 0)'
    SetErrorLevel 0
SectionEnd
