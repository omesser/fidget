# Shared keyboard SendInput for the Windows leaves. The MOUSEINPUT arm sizes
# INPUT to sizeof(INPUT), 40 bytes on x64; a 32-byte cbSize fails every event
# with error 87. Error 5 is UIPI: a foreground window above our integrity level.

if (-not ("SendInputWin" -as [type])) {
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class SendInputWin {
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; [FieldOffset(8)] public MOUSEINPUT m; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder t, int n);
    public static int InputSize { get { return Marshal.SizeOf(typeof(INPUT)); } }
    public static string Refusal(uint sent, int count, int error, string title, uint pid) {
        return "SendInput sent " + sent + " of " + count + " events, error " + error + "; foreground '" + title + "' pid " + pid;
    }
    static void Send(INPUT[] all) {
        uint sent = SendInput((uint)all.Length, all, InputSize);
        if (sent == all.Length) return;
        int error = Marshal.GetLastWin32Error();
        IntPtr front = GetForegroundWindow();
        uint pid = 0;
        var title = new StringBuilder(512);
        if (front != IntPtr.Zero) { GetWindowThreadProcessId(front, out pid); GetWindowText(front, title, title.Capacity); }
        throw new InvalidOperationException(Refusal(sent, all.Length, error, title.ToString(), pid));
    }
    public static void Key(ushort vk, bool up) {
        var i = new INPUT();
        i.type = 1;
        i.k.vk = vk;
        i.k.flags = up ? 2u : 0u;
        Send(new INPUT[] { i });
    }
}
"@
}
