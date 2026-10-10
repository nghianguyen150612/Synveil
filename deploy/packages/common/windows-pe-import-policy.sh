#!/usr/bin/env bash
# Windows system DLL imports supplied by supported Windows installations.
# NCRYPT.DLL is the inbox Windows CNG API library (Ncrypt.lib), available on
# the project's supported Windows 11 installation target.
# CABINET.DLL is the inbox Cabinet API library, present on Windows 11 and
# imported by Microsoft's VC++ redistributable installer itself.
# MSI.DLL is the Windows Installer API library supplied by Windows 11 and
# imported by Microsoft's VC++ redistributable installer itself.
# WININET.DLL is the Windows Internet API library supplied by Windows 11 and
# imported by Microsoft's VC++ redistributable installer itself.
# SPDX-License-Identifier: MIT

is_system_dll() {
    local name="${1^^}"
    case "$name" in
        API-MS-WIN-*|EXT-MS-WIN-*|KERNEL32.DLL|KERNELBASE.DLL|NTDLL.DLL|ADVAPI32.DLL|\
        USER32.DLL|GDI32.DLL|OLE32.DLL|OLEAUT32.DLL|SHELL32.DLL|SHLWAPI.DLL|COMDLG32.DLL|\
        COMBASE.DLL|WS2_32.DLL|IPHLPAPI.DLL|CRYPT32.DLL|BCRYPT.DLL|BCRYPTPRIMITIVES.DLL|WINHTTP.DLL|\
        VERSION.DLL|DWMAPI.DLL|IMM32.DLL|SETUPAPI.DLL|AUTHZ.DLL|NCRYPT.DLL|D3D11.DLL|D3D12.DLL|\
        D3D9.DLL|DNSAPI.DLL|DWRITE.DLL|DXGI.DLL|IMAGEHLP.DLL|MPR.DLL|MSVCRT.DLL|\
        NETAPI32.DLL|RPCRT4.DLL|SECUR32.DLL|SHCORE.DLL|USERENV.DLL|UXTHEME.DLL|\
        UIAUTOMATIONCORE.DLL|WINMM.DLL|WINSPOOL.DRV|WTSAPI32.DLL|CABINET.DLL|MSI.DLL|WININET.DLL)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}
