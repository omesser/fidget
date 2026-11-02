# UI Automation change events as one waitable signal: [FidgetUiaSignal]::new($pid)
# (0 = any process), .Wait($ms) is true on a window, menu, structure, name,
# bounds or focus change. Wait-UiaChange re-runs a check on each one.
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }

# A UI Automation call has no timeout, so a stuck one held a leaf to the job's
# 3 min cap with no word of where (#1765). Steps go on a trail; Arm names the
# last ones, kills Fidget's tree, and ends the leaf if it stalls.
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
    public static void Mark(string step) {
        lock (gate) {
            steps.Enqueue(clock.ElapsedMilliseconds + " ms " + step);
            if (steps.Count > 24) { steps.Dequeue(); }
        }
    }
    public static string Dump() {
        lock (gate) { return string.Join("\n  ", steps.ToArray()); }
    }
    public static void Arm(int seconds, int fidgetPid) {
        Mark("Fidget started, pid " + fidgetPid);
        if (timer != null) { timer.Dispose(); }
        timer = new Timer(delegate {
            Console.Error.WriteLine("FAIL: the leaf stalled " + seconds + " s after Fidget started; its last steps:\n  " + Dump());
            Console.Error.Flush();
            try {
                ProcessStartInfo kill = new ProcessStartInfo("taskkill.exe", "/PID " + fidgetPid + " /T /F");
                kill.UseShellExecute = false;
                kill.CreateNoWindow = true;
                Process.Start(kill).WaitForExit(10000);
            } catch (Exception) {
            }
            // A call blocked on Fidget may return now and fail the leaf itself.
            Thread.Sleep(5000);
            Process.GetCurrentProcess().Kill();
        }, null, seconds * 1000, Timeout.Infinite);
    }
}
"@
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase @"
using System;
using System.Threading;
using System.Windows.Automation;
public class FidgetUiaSignal : IDisposable {
    readonly AutoResetEvent changed = new AutoResetEvent(false);
    readonly int pid;
    readonly AutomationEventHandler onEvent;
    readonly StructureChangedEventHandler onStructure;
    readonly AutomationPropertyChangedEventHandler onProperty;
    readonly AutomationFocusChangedEventHandler onFocus;
    static readonly AutomationEvent[] Events = {
        WindowPattern.WindowOpenedEvent, WindowPattern.WindowClosedEvent,
        AutomationElement.MenuOpenedEvent, AutomationElement.MenuClosedEvent,
    };
    public FidgetUiaSignal(int processId) {
        pid = processId;
        onEvent = (s, e) => Fire(s);
        onStructure = (s, e) => Fire(s);
        onProperty = (s, e) => Fire(s);
        onFocus = (s, e) => Fire(s);
        var root = AutomationElement.RootElement;
        foreach (var ev in Events) {
            Automation.AddAutomationEventHandler(ev, root, TreeScope.Subtree, onEvent);
        }
        Automation.AddStructureChangedEventHandler(root, TreeScope.Subtree, onStructure);
        Automation.AddAutomationPropertyChangedEventHandler(root, TreeScope.Subtree, onProperty,
            AutomationElement.NameProperty, AutomationElement.BoundingRectangleProperty);
        // Focus moves with the foreground window, which fires nothing above.
        Automation.AddAutomationFocusChangedEventHandler(onFocus);
    }
    void Fire(object sender) {
        if (pid != 0) {
            try {
                var el = sender as AutomationElement;
                if (el != null && el.Current.ProcessId != pid) return;
            } catch (ElementNotAvailableException) {
                // Gone already: a close is still a change worth a re-check.
            }
        }
        changed.Set();
    }
    public bool Wait(int ms) { return ms > 0 && changed.WaitOne(ms); }
    public void Dispose() {
        foreach (var ev in Events) {
            Automation.RemoveAutomationEventHandler(ev, AutomationElement.RootElement, onEvent);
        }
        Automation.RemoveStructureChangedEventHandler(AutomationElement.RootElement, onStructure);
        Automation.RemoveAutomationPropertyChangedEventHandler(AutomationElement.RootElement, onProperty);
        Automation.RemoveAutomationFocusChangedEventHandler(onFocus);
        changed.Dispose();
    }
}
"@

# Run a check now and again on each UI Automation change until it returns
# true or the seconds run out. A caller's own $Shared signal is reused, not disposed.
function Wait-UiaChange([int]$ProcessId, [double]$Seconds, [scriptblock]$Check, $Shared = $null) {
    # The clock starts once the handlers are registered, which can be slow.
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
