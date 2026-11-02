# UI changes as one waitable signal: [FidgetUiaSignal]::new($pid) (0 = any
# process), .Wait($ms) is true on a window, menu, structure, name, bounds or
# focus change. Wait-UiaChange re-runs a UI Automation check on each one.
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }

# A UI Automation call has no timeout, so a stuck one held a leaf to the job's
# 3 min cap with no word of where (#1765). Steps go on a trail; once armed, a
# leaf with no new step for the budget prints them, kills Fidget, and ends.
if (-not ("FidgetTrail" -as [type])) {
    Add-Type @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
public static class FidgetTrail {
    static readonly object gate = new object();
    static readonly Queue<string> steps = new Queue<string>();
    static readonly Stopwatch clock = Stopwatch.StartNew();
    static Timer timer;
    static int budgetMs;
    public static void Mark(string step) {
        lock (gate) {
            steps.Enqueue(clock.ElapsedMilliseconds + " ms " + step);
            if (steps.Count > 24) { steps.Dequeue(); }
            if (timer != null) { timer.Change(budgetMs, Timeout.Infinite); }
        }
    }
    public static string Dump() {
        lock (gate) { return string.Join("\n  ", steps.ToArray()); }
    }
    public static void Arm(int seconds, int fidgetPid) {
        lock (gate) {
            if (timer != null) { timer.Dispose(); }
            budgetMs = seconds * 1000;
            timer = new Timer(delegate { Stall(seconds, fidgetPid); });
        }
        Mark("Fidget started, pid " + fidgetPid);
    }
    static void Stall(int seconds, int fidgetPid) {
        Console.Error.WriteLine("FAIL: the leaf took no step for " + seconds + " s; its last steps:\n  " + Dump());
        Console.Error.Flush();
        try {
            ProcessStartInfo kill = new ProcessStartInfo("taskkill.exe", "/PID " + fidgetPid + " /T /F");
            kill.UseShellExecute = false;
            kill.CreateNoWindow = true;
            Process.Start(kill).WaitForExit(10000);
        } catch (Exception) {
        }
        // A call blocked on Fidget may return now and fail the leaf itself.
        // Else exit hard: no finally runs; a manual run keeps its temp files.
        Thread.Sleep(5000);
        Process.GetCurrentProcess().Kill();
    }
}
"@
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
# WinEvent hooks, not UI Automation handlers: adding or removing a handler on
# the root waits on every provider, 2 s per unresponsive one and sometimes
# forever (#1765). An out-of-context hook never calls into another process.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Threading;
public class FidgetUiaSignal : IDisposable {
    delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }
    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint min, uint max, IntPtr module, WinEventProc proc, uint pid, uint tid, uint flags);
    [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
    [DllImport("user32.dll")] static extern int GetMessage(out MSG msg, IntPtr hwnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool PeekMessage(out MSG msg, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool PostThreadMessage(uint tid, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    const uint OutOfContextSkipOwn = 0x0000 | 0x0002;
    const int ObjidCursor = -9;
    readonly AutoResetEvent changed = new AutoResetEvent(false);
    readonly uint pid;
    readonly WinEventProc proc;
    readonly Thread pump;
    uint pumpId;
    // Foreground, menu and popup start and end; object create through name
    // change, which spans show, hide, reorder, focus, state and location.
    public FidgetUiaSignal(int processId) {
        pid = (uint)processId;
        proc = OnEvent;
        var ready = new ManualResetEvent(false);
        pump = new Thread(delegate() {
            MSG msg;
            PeekMessage(out msg, IntPtr.Zero, 0, 0, 0);
            pumpId = GetCurrentThreadId();
            IntPtr system = SetWinEventHook(0x0003, 0x0007, IntPtr.Zero, proc, 0, 0, OutOfContextSkipOwn);
            IntPtr objects = SetWinEventHook(0x8000, 0x800C, IntPtr.Zero, proc, 0, 0, OutOfContextSkipOwn);
            ready.Set();
            while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) { }
            UnhookWinEvent(system);
            UnhookWinEvent(objects);
        });
        pump.IsBackground = true;
        pump.Start();
        ready.WaitOne();
    }
    void OnEvent(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
        if (idObject == ObjidCursor) return;
        if (pid != 0) {
            // WebView2's windows belong to its own processes; their root is ours.
            uint owner;
            GetWindowThreadProcessId(GetAncestor(hwnd, 2), out owner);
            if (owner != pid) return;
        }
        changed.Set();
    }
    public bool Wait(int ms) { return ms > 0 && changed.WaitOne(ms); }
    public void Dispose() {
        PostThreadMessage(pumpId, 0x0012, IntPtr.Zero, IntPtr.Zero);
        // Not disposing changed: a hook still in flight past the join would
        // throw on its pump thread, and that ends the whole shell.
        pump.Join(1000);
    }
}
"@

# Run a check now and again on each UI change until it returns
# true or the seconds run out. A caller's own $Shared signal is reused, not disposed.
function Wait-UiaChange([int]$ProcessId, [double]$Seconds, [scriptblock]$Check, $Shared = $null) {
    [FidgetTrail]::Mark("uia: register")
    $signal = if ($Shared) { $Shared } else { [FidgetUiaSignal]::new($ProcessId) }
    [FidgetTrail]::Mark("uia: wait $Seconds s")
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    try {
        while ($true) {
            $result = & $Check
            if ($result) { return $result }
            $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
            if (-not $signal.Wait($left)) { return (& $Check) }
        }
    } finally {
        [FidgetTrail]::Mark("uia: unregister")
        if (-not $Shared) { $signal.Dispose() }
    }
}
