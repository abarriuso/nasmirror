# File Explorer's copy engine: SHFileOperation, the shell copy behind
# Explorer's copy and paste. Dot-sourced by bench.ps1 and locked.ps1.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ShellCopy {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct SHFILEOPSTRUCT {
        public IntPtr hwnd;
        public uint wFunc;
        public string pFrom;
        public string pTo;
        public ushort fFlags;
        [MarshalAs(UnmanagedType.Bool)] public bool fAnyOperationsAborted;
        public IntPtr hNameMappings;
        public string lpszProgressTitle;
    }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHFileOperation(ref SHFILEOPSTRUCT op);
    // FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOCONFIRMMKDIR | FOF_NOERRORUI:
    // no progress window, "replace" on every conflict, errors returned, not shown.
    public static int Copy(string from, string to, out bool aborted) {
        var op = new SHFILEOPSTRUCT();
        op.wFunc = 2; // FO_COPY
        op.pFrom = from + "\0\0";
        op.pTo = to + "\0\0";
        op.fFlags = 0x614;
        int r = SHFileOperation(ref op);
        aborted = op.fAnyOperationsAborted;
        return r;
    }
}
'@
