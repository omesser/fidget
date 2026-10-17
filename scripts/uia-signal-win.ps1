# UI Automation change events as one waitable signal: [FidgetUiaSignal]::new($pid)
# (0 = any process), .Wait($ms) is true on a window, menu, structure, name,
# bounds or focus change. Wait-UiaChange re-runs a check on each one.
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }
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
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    $signal = if ($Shared) { $Shared } else { [FidgetUiaSignal]::new($ProcessId) }
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
