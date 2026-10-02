#!/usr/bin/env python3
"""List the DLLs a Windows executable imports and fail if any is not a Windows system DLL.

    python scripts/check-imports.py path/to/cutemarkdown.exe

Pure standard library (works on Linux for cross builds and on Windows CI). Covers the normal
import table and the delay-load table. Exit status 1 means "this exe needs something that
is not part of Windows" (VC++ runtime, MinGW runtime, ...), i.e. it is not standalone.
"""
import struct
import sys

# DLLs that ship with every supported Windows (10/11), lower-case.
SYSTEM_DLLS = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "cfgmgr32.dll", "combase.dll",
    "comctl32.dll", "comdlg32.dll", "crypt32.dll", "d2d1.dll", "d3d11.dll", "d3d12.dll",
    "d3dcompiler_47.dll", "dcomp.dll", "dwmapi.dll", "dwrite.dll", "dxgi.dll", "gdi32.dll",
    "imm32.dll", "iphlpapi.dll", "kernel32.dll", "kernelbase.dll", "msimg32.dll", "msvcrt.dll",
    "ncrypt.dll", "netapi32.dll", "normaliz.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
    "opengl32.dll", "propsys.dll", "rpcrt4.dll", "sechost.dll", "secur32.dll", "setupapi.dll",
    "shcore.dll", "shell32.dll", "shlwapi.dll", "uiautomationcore.dll", "ucrtbase.dll",
    "user32.dll", "userenv.dll", "uxtheme.dll", "version.dll", "win32u.dll", "winmm.dll",
    "wintrust.dll", "ws2_32.dll", "wtsapi32.dll",
}
SYSTEM_PREFIXES = ("api-ms-win-", "ext-ms-win-")  # API sets, resolved by the OS loader


def rva_to_offset(sections, rva):
    for va, vsize, raw_ptr, raw_size in sections:
        if va <= rva < va + max(vsize, raw_size):
            return rva - va + raw_ptr
    raise ValueError(f"RVA {rva:#x} is not inside any section")


def cstr(data, off):
    end = data.index(b"\0", off)
    return data[off:end].decode("ascii", "replace")


def imported_dlls(path):
    data = open(path, "rb").read()
    if data[:2] != b"MZ":
        raise ValueError("not a PE file (no MZ header)")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file (no PE signature)")
    nsections, = struct.unpack_from("<H", data, pe + 6)
    opt_size, = struct.unpack_from("<H", data, pe + 20)
    opt = pe + 24
    magic, = struct.unpack_from("<H", data, opt)
    dirs = opt + (112 if magic == 0x20B else 96)  # PE32+ / PE32
    sec = opt + opt_size
    sections = []
    for i in range(nsections):
        vsize, va, raw_size, raw_ptr = struct.unpack_from("<IIII", data, sec + 40 * i + 8)
        sections.append((va, vsize, raw_ptr, raw_size))

    names = []
    # (data directory index, descriptor size, offset of the name RVA inside a descriptor)
    for index, desc_size, name_at in ((1, 20, 12), (13, 32, 4)):
        rva, size = struct.unpack_from("<II", data, dirs + 8 * index)
        if not rva:
            continue
        off = rva_to_offset(sections, rva)
        while True:
            desc = data[off:off + desc_size]
            if len(desc) < desc_size or not any(desc):
                break
            name_rva, = struct.unpack_from("<I", desc, name_at)
            if name_rva:
                names.append(cstr(data, rva_to_offset(sections, name_rva)))
            off += desc_size
    return names


def is_system(name):
    n = name.lower()
    return n in SYSTEM_DLLS or n.startswith(SYSTEM_PREFIXES)


def main():
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <file.exe>")
    try:
        found = imported_dlls(sys.argv[1])
    except (OSError, ValueError, struct.error) as e:
        sys.exit(f"error: {sys.argv[1]}: {e}")
    names = sorted({n.lower(): n.lower() for n in found}.values())
    bad = [n for n in names if not is_system(n)]
    print(f"{len(names)} imported DLLs:")
    for n in names:
        print(f"  {'OK ' if is_system(n) else 'BAD'} {n}")
    if bad:
        print(f"\nERROR: not a Windows system DLL: {', '.join(bad)}", file=sys.stderr)
        print("The executable must not depend on the VC++ or MinGW runtimes.", file=sys.stderr)
        sys.exit(1)
    print("\nOnly Windows system DLLs are imported.")


if __name__ == "__main__":
    main()
