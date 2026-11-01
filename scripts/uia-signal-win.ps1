# UI Automation change events as one waitable signal: [FidgetUiaSignal]::new($pid)
# (0 = any process). .Wait($ms) is true on a window, menu, structure, name,
# bounds or focus change. AlsoFile wakes the same wait when that file is written.
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase, System @"
using System;
using System.IO;
using System.Threading;
using System.Windows.Automation;
public class FidgetUiaSignal : IDisposable {
    readonly AutoResetEvent changed = new AutoResetEvent(false);
    readonly int pid;
    readonly AutomationEventHandler onEvent;
    readonly StructureChangedEventHandler onStructure;
    readonly AutomationPropertyChangedEventHandler onProperty;
    readonly AutomationFocusChangedEventHandler onFocus;
    FileSystemWatcher logWatch;
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
    // A write to this file is not a UI Automation event. Same wait either way.
    public void Also(string file) {
        logWatch = new FileSystemWatcher(Path.GetDirectoryName(file), Path.GetFileName(file));
        logWatch.NotifyFilter = NotifyFilters.FileName | NotifyFilters.Size | NotifyFilters.LastWrite;
        FileSystemEventHandler on = delegate { changed.Set(); };
        logWatch.Created += on;
        logWatch.Changed += on;
        logWatch.EnableRaisingEvents = true;
    }
    public bool Wait(int ms) { return ms > 0 && changed.WaitOne(ms); }
    public void Dispose() {
        foreach (var ev in Events) {
            Automation.RemoveAutomationEventHandler(ev, AutomationElement.RootElement, onEvent);
        }
        Automation.RemoveStructureChangedEventHandler(AutomationElement.RootElement, onStructure);
        Automation.RemoveAutomationPropertyChangedEventHandler(AutomationElement.RootElement, onProperty);
        Automation.RemoveAutomationFocusChangedEventHandler(onFocus);
        if (logWatch != null) {
            logWatch.EnableRaisingEvents = false;
            logWatch.Dispose();
        }
        changed.Dispose();
    }
}
"@

# Run a check now and again on each UI Automation change until it returns
# true or the seconds run out. A caller's own $Shared signal is reused, not
# disposed. AlsoFile, when this call owns the signal, wakes it on a write.
function Wait-UiaChange([int]$ProcessId, [double]$Seconds, [scriptblock]$Check, $Shared = $null, [string]$AlsoFile = "") {
    # The clock starts once the handlers are registered, which can be slow.
    $signal = if ($Shared) { $Shared } else { [FidgetUiaSignal]::new($ProcessId) }
    if ($AlsoFile -and -not $Shared) { $signal.Also($AlsoFile) }
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    try {
        while ($true) {
            $result = & $Check
            if ($result) { return $result }
            $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
            if (-not $signal.Wait($left)) { return (& $Check) }
        }
    } finally {
        if (-not $Shared) { $signal.Dispose() }
    }
}
